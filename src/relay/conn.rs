use super::*;
use bytes::BytesMut;

use log::{info, log_enabled, warn};
use std::io;
use std::sync::Arc;
use std::time::Duration;

use crate::map;
use crate::map::*;
use crate::net;

use crate::net::Stream;
use crate::net::CID;

/// mock of  handle_conn, utilize handle_in_accumulate_result and  OutSelector
pub async fn handle_conn_clonable<'a, T, T2>(
    in_conn: net::Conn,
    ins_iterator: T,
    selector: &'a dyn OutSelector<'a, T2>,
    ti: Option<Arc<net::TransmissionInfo>>,
) -> io::Result<()>
where
    T: Iterator<Item = &'a MapperBox>,
    T2: Iterator<Item = &'a MapperBox>,
{
    let cid = match ti.as_ref() {
        Some(ti) => CID::new_ordered(&ti.alive_connection_count),
        None => CID::new(),
    };

    let cidc = cid.clone();
    let listen_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            map::accumulate::<_>(
                cidc,
                ProxyBehavior::DECODE,
                MapResult::c(in_conn),
                ins_iterator,
            )
            .await
        })
        .await;

    let listen_result = match listen_result {
        Ok(lr) => lr,
        Err(e) => {
            warn!("{cid}, handshake in server failed with io::Error, {e}");

            return Err(e.into());
        }
    };

    handle_in_accumulate_result(listen_result, selector, ti).await
}

pub async fn cp_stream(
    cid: CID,
    s1: Stream,
    s2: Stream,
    ed: Option<BytesMut>, //earlydata
    ti: Option<Arc<net::TransmissionInfo>>,
) {
    match (s1, s2) {
        (Stream::TCP(i), Stream::TCP(o)) => cp_tcp::cp_conn(cid, i, o, ed, ti).await,
        (Stream::TCP(i), Stream::UDP(o)) => {
            let _ = cp_udp::cp_udp_tcp(cid, o, i, false, ed, ti).await;
        }
        (Stream::UDP(i), Stream::TCP(o)) => {
            let _ = cp_udp::cp_udp_tcp(cid, i, o, true, ed, ti).await;
        }
        (Stream::UDP(i), Stream::UDP(o)) => cp_udp(cid, i, o, ti).await,
        _ => {
            warn!("can't cp stream when either of them is None");
        }
    }
}

pub async fn cp_udp(
    cid: CID,
    in_conn: net::addr_conn::AddrConn,
    out_conn: net::addr_conn::AddrConn,
    ti: Option<Arc<net::TransmissionInfo>>,
) {
    info!("cid: {cid}, relay udp start",);

    //discard early data, as we don't know it's target addr

    let tic = ti.clone();
    scopeguard::defer! {

        if let Some(ti) = tic {
            ti.alive_connection_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        }
        info!("cid: {cid},udp relay end" );
    }

    let _ = net::addr_conn::cp(cid.clone(), in_conn, out_conn, ti).await;
}

/// Send + Sync to use in async
pub trait OutSelector<'a, T>: Send + Sync
where
    T: Iterator<Item = &'a MapperBox>,
{
    fn select(&self, params: Vec<Option<AnyData>>) -> T;
}

pub struct FixedOutSelector<'a, T>
where
    T: Iterator<Item = &'a MapperBox> + Clone + Send,
{
    pub mappers: T,
}

impl<'a, T> OutSelector<'a, T> for FixedOutSelector<'a, T>
where
    T: Iterator<Item = &'a MapperBox> + Clone + Send + Sync,
{
    fn select(&self, _params: Vec<Option<AnyData>>) -> T {
        self.mappers.clone()
    }
}

pub async fn handle_in_accumulate_result<'a, T, T2>(
    mut listen_result: AccumulateResult<'a, T>,

    out_selector: &'a dyn OutSelector<'a, T2>,

    ti: Option<Arc<net::TransmissionInfo>>,
) -> io::Result<()>
where
    T: Iterator<Item = &'a MapperBox>,
    T2: Iterator<Item = &'a MapperBox>,
{
    let cid = &listen_result.id.unwrap();
    let target_addr = match listen_result.a.take() {
        Some(ta) => ta,
        None => {
            warn!(
                "{}, handshake in server succeed but got no target_addr",
                cid
            );
            let _ = listen_result.c.try_shutdown().await;
            return Err(io::Error::other(
                "handshake in server succeed but got no target_addr",
            ));
        }
    };
    if log_enabled!(log::Level::Info) {
        info!(
            "{cid}, handshake in server succeed, target_addr: {}",
            &target_addr
        )
    }

    let outc_iterator = out_selector.select(listen_result.d);

    let cidc = cid.clone();
    let dial_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            map::accumulate::<_>(
                cidc,
                ProxyBehavior::ENCODE,
                MapResult {
                    a: Some(target_addr),
                    b: listen_result.b,
                    c: Stream::None,
                    d: None,
                    e: None,
                    new_id: None,
                },
                outc_iterator,
            )
            .await
        })
        .await;

    if let Err(e) = dial_result {
        warn!("{cid}, dial out client timeout, {e}",);
        return Err(e.into());
    }
    let dial_result = dial_result.unwrap();
    if let Some(e) = dial_result.e {
        warn!("{cid}, dial out client failed, {e}",);
        return Err(e);
    } else if let Stream::None = dial_result.c {
        warn!("{cid}, dial out client stream got consumed ",);

        return Ok(());
    }

    if let Some(rta) = dial_result.a {
        warn!("{cid}, dial out client succeed, but the target_addr is not consumed, {rta} ",);
    }
    cp_stream(cid.clone(), listen_result.c, dial_result.c, None, ti).await;

    Ok(())
}
