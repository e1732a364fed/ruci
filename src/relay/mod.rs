/*!
 * relay 包定义了一种转发逻辑，但是它不是强制性的，可用于参考。
具体实现 中可以有不同的转发逻辑

*/
pub mod cp_tcp;
pub mod cp_udp;
pub mod route;

use std::sync::Arc;

use bytes::BytesMut;
use log::{info, log_enabled, warn};

use crate::net::{self, Stream, CID};

use self::acc::MIterBox;
use self::route::OutSelector;

use anyhow::anyhow;
use std::time::Duration;

use crate::map::*;

pub const READ_HANDSHAKE_TIMEOUT: u64 = 15; // 15秒的最长握手等待时间。 //todo: 修改这里

/// block until in and out handshake is over.
/// utilize handle_in_accumulate_result and  route::OutSelector
pub async fn handle_in_stream(
    in_conn: Stream,
    ins_iterator: MIterBox,
    out_selector: Arc<Box<dyn OutSelector>>,
    ti: Option<Arc<net::TransmissionInfo>>,
) -> anyhow::Result<()> {
    let cid = match ti.as_ref() {
        Some(ti) => CID::new_ordered(&ti.alive_connection_count),
        None => CID::new(),
    };

    let cidc = cid.clone();
    let listen_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            acc::accumulate(
                cidc,
                ProxyBehavior::DECODE,
                MapResult::builder().c(in_conn).build(),
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

    handle_in_accumulate_result(listen_result, out_selector, ti).await
}

/// block until out handshake is over
pub async fn handle_in_accumulate_result(
    mut listen_result: acc::AccumulateResult,

    out_selector: Arc<Box<dyn OutSelector>>,

    ti: Option<Arc<net::TransmissionInfo>>,
) -> anyhow::Result<()> {
    let cid = listen_result.id;
    let target_addr = match listen_result.a.take() {
        Some(ta) => ta,
        None => {
            let e = anyhow!(
                "{cid}, handshake in server succeed but got no target_addr, e: {:?}",
                listen_result.e
            );
            warn!("{}", e);
            let _ = listen_result.c.try_shutdown().await;
            return Err(e);
        }
    };
    if log_enabled!(log::Level::Info) {
        match listen_result.b.as_ref() {
            Some(ed) => {
                info!(
                    "{cid}, handshake in server succeed with ed, target_addr: {}, ed {}",
                    &target_addr,
                    ed.len()
                )
            }
            None => {
                info!(
                    "{cid}, handshake in server succeed, target_addr: {}",
                    &target_addr,
                )
            }
        }
    }

    let outbound = out_selector
        .select(&target_addr, &listen_result.chain_tag, &listen_result.d)
        .await;

    let cidc = cid.clone();
    let dial_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            acc::accumulate(
                cidc,
                ProxyBehavior::ENCODE,
                MapResult {
                    a: Some(target_addr),
                    b: listen_result.b,
                    ..Default::default()
                },
                outbound,
            )
            .await
        })
        .await;

    let dial_result = match dial_result {
        Ok(d) => d,
        Err(e) => {
            warn!("{cid}, dial out client timeout, {e}",);
            return Err(e.into());
        }
    };
    let cid = dial_result.id;

    if let Some(e) = dial_result.e {
        warn!("{cid}, dial out client failed, {e}",);
        return Err(e);
    }
    if let Stream::None = dial_result.c {
        warn!("{cid}, dial out client stream got consumed ",);

        return Ok(());
    }

    if let Some(rta) = dial_result.a {
        warn!("{cid}, dial out client succeed, but the target_addr is not consumed, {rta} ",);
    }
    cp_stream(cid, listen_result.c, dial_result.c, dial_result.b, ti);

    Ok(())
}

/// non-blocking,
pub fn cp_stream(
    cid: CID,
    s1: Stream,
    s2: Stream,
    ed: Option<BytesMut>, //earlydata
    ti: Option<Arc<net::TransmissionInfo>>,
) {
    match (s1, s2) {
        (Stream::TCP(i), Stream::TCP(o)) => cp_tcp::cp_conn(cid, i, o, ed, ti),
        (Stream::TCP(i), Stream::UDP(o)) => {
            tokio::spawn(cp_udp::cp_udp_tcp(cid, o, i, false, ed, ti));
        }
        (Stream::UDP(i), Stream::TCP(o)) => {
            tokio::spawn(cp_udp::cp_udp_tcp(cid, i, o, true, ed, ti));
        }
        (Stream::UDP(i), Stream::UDP(o)) => {
            tokio::spawn(cp_udp(cid, i, o, ti));
        }
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
    info!("{cid}, relay udp start",);

    //discard early data, as we don't know it's target addr

    let tic = ti.clone();
    scopeguard::defer! {

        if let Some(ti) = tic {
            ti.alive_connection_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        }
        info!("{cid},udp relay end" );
    }

    let _ = net::addr_conn::cp(cid.clone(), in_conn, out_conn, ti).await;
}
