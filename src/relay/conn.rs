use self::acc::MIterBox;
use self::route::OutSelector;

use super::*;

use anyhow::anyhow;
use log::{info, log_enabled, warn};
use std::sync::Arc;
use std::time::Duration;

use crate::map::*;
use crate::net;

use crate::net::Stream;
use crate::net::CID;

/// block until in and out handshake is over.
/// utilize handle_in_accumulate_result and  route::OutSelector
pub async fn handle_conn(
    in_conn: net::Conn,
    ins_iterator: MIterBox,
    selector: Arc<Box<dyn OutSelector>>,
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
    mut listen_result: acc::AccumulateResult,

    out_selector: Arc<Box<dyn OutSelector>>,

    ti: Option<Arc<net::TransmissionInfo>>,
) -> anyhow::Result<()> {
    let cid = listen_result
        .id
        .as_ref()
        .expect("listen_result contains an id");
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
