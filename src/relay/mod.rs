/*!
 * relay 包定义了一种转发逻辑，但是它不是强制性的，可用于参考。
具体实现 中可以有不同的转发逻辑

*/
pub mod cp_tcp;
pub mod cp_udp;
pub mod record;
pub mod route;

pub use record::*;

use std::sync::Arc;

use bytes::BytesMut;
use tracing::{debug, info, warn};

use crate::net::addr_conn::AsyncWriteAddrExt;
use crate::net::{self, Addr, Stream, CID};

use self::acc::{AccumulateParams, DMIterBox};
use self::route::OutSelector;

use anyhow::anyhow;
use std::time::Duration;

use crate::map::*;

pub const READ_HANDSHAKE_TIMEOUT: u64 = 15; // 15秒的最长握手等待时间。 //todo: 修改这里

/// block until in and out handshake is over.
/// utilize handle_in_accumulate_result and  route::OutSelector
pub async fn handle_in_stream(
    in_conn: Stream,
    ins_iterator: DMIterBox,
    out_selector: Arc<Box<dyn OutSelector>>,
    gtr: Option<Arc<net::GlobalTrafficRecorder>>,

    newc_recorder: OptNewInfoSender,

    #[cfg(feature = "trace")] updater: net::OptUpdater,
) -> anyhow::Result<()> {
    let cid = match gtr.as_ref() {
        Some(gtr) => CID::new_ordered(&gtr.alive_connection_count),
        None => CID::new_random(),
    };

    let cidc = cid.clone();
    let listen_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            acc::accumulate(AccumulateParams {
                cid: cidc,
                behavior: ProxyBehavior::DECODE,
                initial_state: MapResult::builder().c(in_conn).build(),
                mappers: ins_iterator,

                #[cfg(feature = "trace")]
                trace: Vec::new(),
            })
            .await
        })
        .await;

    let listen_result = match listen_result {
        Ok(lr) => lr,
        Err(e) => {
            warn!(
                cid = %cid,
                "handshake inbound failed with io::Error, {e}"
            );

            return Err(e.into());
        }
    };

    handle_in_accumulate_result(
        listen_result,
        out_selector,
        gtr,
        newc_recorder,
        #[cfg(feature = "trace")]
        updater,
    )
    .await
}

/// block until out handshake is over
pub async fn handle_in_accumulate_result(
    mut listen_result: acc::AccumulateResult,

    out_selector: Arc<Box<dyn OutSelector>>,

    tr: Option<Arc<net::GlobalTrafficRecorder>>,

    newc_recorder: OptNewInfoSender,

    #[cfg(feature = "trace")] updater: net::OptUpdater,
) -> anyhow::Result<()> {
    let cid = listen_result.id;
    let target_addr = match listen_result.a.take() {
        Some(ta) => ta,
        None => {
            let return_e: anyhow::Error;
            match listen_result.e {
                Some(err) => {
                    return_e = anyhow!("handshake inbound failed with Error: {:#?}", err);

                    warn!(cid = %cid, "{}", return_e);
                    let _ = listen_result.c.try_shutdown().await;
                    return Err(return_e);
                }
                None => match &listen_result.c {
                    Stream::None => {
                        return_e = anyhow!("handshake inbound ok and stream got consumed");
                        info!(cid = %cid, "{}", return_e);
                        return Ok(());
                    }
                    _ => {
                        return_e =
                            anyhow!( "handshake inbound succeed but got no target_addr, will use empty target_addr");
                        warn!(cid = %cid, "{}", return_e);
                        Addr::default()
                    }
                },
            }
        }
    };
    if tracing::enabled!(tracing::Level::INFO) {
        match listen_result.b.as_ref() {
            Some(ed) => {
                info!(
                    cid = %cid,
                    "handshake inbound succeed with ed, target_addr: {}, ed {}",
                    &target_addr,
                    ed.len()
                )
            }
            None => {
                info!(
                    cid = %cid,
                    target_addr = %target_addr,
                    "handshake inbound succeed",
                )
            }
        }
    }

    let outbound = out_selector
        .select(&target_addr, &listen_result.chain_tag, &listen_result.d)
        .await;

    let cidc = cid.clone();
    let ta_clone = target_addr.clone();
    let dial_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            acc::accumulate(AccumulateParams {
                cid: cidc,
                behavior: ProxyBehavior::ENCODE,
                initial_state: MapResult {
                    a: Some(ta_clone),
                    b: listen_result.b,
                    ..Default::default()
                },
                mappers: outbound,

                #[cfg(feature = "trace")]
                trace: Vec::new(),
            })
            .await
        })
        .await;

    let dial_result = match dial_result {
        Ok(d) => d,
        Err(e) => {
            warn!(cid = %cid, "dial out client timeout, {e}",);
            return Err(e.into());
        }
    };
    let cid = dial_result.id;

    if let Some(e) = dial_result.e {
        warn!(cid = %cid, "dial out client failed, {:#}", e);
        return Err(e);
    }
    if let Stream::None = dial_result.c {
        warn!(
            cid = %cid,
            "dial out client stream got consumed ",
        );

        return Ok(());
    }

    if let Some(rta) = &dial_result.a {
        if rta.eq(&Addr::default()) {
            debug!(
                cid = %cid,
                "dial out client succeed with empty target_addr left",
            );
        } else {
            debug!( cid = %cid,
            "dial out client succeed, but the target_addr is not consumed, might be udp first target addr: {rta} ",);
        }
    }

    if let Some(r) = newc_recorder {
        let cid = cid.clone();

        r.send(NewConnInfo {
            cid,
            in_tag: listen_result.chain_tag,
            out_tag: dial_result.chain_tag,
            target_addr,

            #[cfg(feature = "trace")]
            in_trace: listen_result.trace,

            #[cfg(feature = "trace")]
            out_trace: dial_result.trace,
        })
        .await?;
    }

    cp_stream(
        cid,
        listen_result.c,
        dial_result.c,
        dial_result.b,
        dial_result.a,
        tr,
        #[cfg(feature = "trace")]
        updater,
    );

    Ok(())
}

/// non-blocking,
pub fn cp_stream(
    cid: CID,
    s1: Stream,
    s2: Stream,
    ed: Option<BytesMut>,            //earlydata
    first_target: Option<net::Addr>, // 用于 udp
    tr: Option<Arc<net::GlobalTrafficRecorder>>,

    #[cfg(feature = "trace")] updater: net::OptUpdater,
) {
    //todo: add trace for udp
    match (s1, s2) {
        (Stream::Conn(i), Stream::Conn(o)) => cp_tcp::cp_conn(
            cid,
            i,
            o,
            ed,
            tr,
            #[cfg(feature = "trace")]
            updater,
        ),
        (Stream::Conn(i), Stream::AddrConn(o)) => {
            tokio::spawn(cp_udp::cp_udp_tcp(cid, o, i, false, ed, first_target, tr));
        }
        (Stream::AddrConn(i), Stream::Conn(o)) => {
            tokio::spawn(cp_udp::cp_udp_tcp(cid, i, o, true, ed, first_target, tr));
        }
        (Stream::AddrConn(i), Stream::AddrConn(o)) => {
            tokio::spawn(cp_udp(cid, i, o, ed, first_target, tr));
        }
        _ => {
            warn!("can't cp stream when one of them is not (Conn or AddrConn)");
        }
    }
}

pub async fn cp_udp(
    cid: CID,
    in_conn: net::addr_conn::AddrConn,
    mut out_conn: net::addr_conn::AddrConn,
    ed: Option<BytesMut>,
    first_target: Option<net::Addr>,
    tr: Option<Arc<net::GlobalTrafficRecorder>>,
) {
    info!(cid = %cid, "relay udp start",);

    let tc = tr.clone();
    scopeguard::defer! {

        if let Some(gtr) = tc {
            gtr.alive_connection_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        }
        info!( cid = %cid,
        "udp relay end" );
    }

    if let Some(real_ed) = ed {
        if let Some(real_first_target) = first_target {
            debug!("cp_udp: writing ed");
            let r = out_conn.w.write(&real_ed, &real_first_target).await;
            if let Err(e) = r {
                warn!("cp_udp: writing ed failed: {e}");
                return;
            }
        } else {
            debug!("cp_udp: writing ed without real_first_target");
            let r = out_conn.w.write(&real_ed, &Addr::default()).await;
            if let Err(e) = r {
                warn!("cp_udp: writing ed failed: {e}");
                return;
            }
        }
    }

    let _ = net::addr_conn::cp(cid.clone(), in_conn, out_conn, tr).await;
}
