use self::route::OutSelector;

use super::*;

use log::{info, log_enabled, warn};
use std::io;
use std::sync::Arc;
use std::time::Duration;

use crate::map;
use crate::map::*;
use crate::net;

use crate::net::Stream;
use crate::net::CID;

/// block until in and out handshake is over.
/// utilize handle_in_accumulate_result and  route::OutSelector
pub async fn handle_conn_clonable(
    in_conn: net::Conn,
    ins_iterator: MIterBox,
    selector: &'static dyn OutSelector,
    ti: Option<Arc<net::TransmissionInfo>>,
) -> io::Result<()> {
    let cid = match ti.as_ref() {
        Some(ti) => CID::new_ordered(&ti.alive_connection_count),
        None => CID::new(),
    };

    let cidc = cid.clone();
    let listen_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            map::accumulate(
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

/// block until out handshake is over
pub async fn handle_in_accumulate_result(
    mut listen_result: AccumulateResult,

    out_selector: &'static dyn OutSelector,

    ti: Option<Arc<net::TransmissionInfo>>,
) -> io::Result<()> {
    let cid = listen_result.id.as_ref().unwrap();
    let target_addr = match listen_result.a.take() {
        Some(ta) => ta,
        None => {
            let e = io::Error::other(format!(
                "{cid}, handshake in server succeed but got no target_addr, e: {:?}",
                listen_result.e
            ));
            warn!("{}", e);
            let _ = listen_result.c.try_shutdown().await;
            return Err(e);
        }
    };
    if log_enabled!(log::Level::Info) {
        match listen_result.b.as_ref() {
            Some(ed) => {
                info!(
                    "{cid}, handshake in server succeed, target_addr: {}, ed {}",
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

    let outc_iterator = out_selector
        .select(&target_addr, &listen_result.chain_tag, &listen_result.d)
        .await;

    let cidc = cid.clone();
    let dial_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            map::accumulate(
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
    }
    if let Stream::None = dial_result.c {
        warn!("{cid}, dial out client stream got consumed ",);

        return Ok(());
    }

    if let Some(rta) = dial_result.a {
        warn!("{cid}, dial out client succeed, but the target_addr is not consumed, {rta} ",);
    }
    cp_stream(
        cid.clone(),
        listen_result.c,
        dial_result.c,
        dial_result.b,
        ti,
    );

    Ok(())
}
