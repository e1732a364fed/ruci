/*!
relay 包定义了一种转发逻辑, 但是它不是强制性的, 可用于参考.
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

use self::fold::{DMIterBox, FoldParams};
use self::route::OutSelector;

use anyhow::anyhow;
use std::time::Duration;

use crate::map::*;

pub const READ_HANDSHAKE_TIMEOUT: u64 = 15; // 15秒的最长握手等待时间.  //todo: 修改这里

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

    let cid_c = cid.clone();
    let listen_result = tokio::time::timeout(
        Duration::from_secs(READ_HANDSHAKE_TIMEOUT),
        fold::fold(FoldParams {
            cid: cid_c,
            behavior: ProxyBehavior::DECODE,
            initial_state: MapResult::builder().c(in_conn).build(),
            mappers: ins_iterator,

            #[cfg(feature = "trace")]
            trace: Vec::new(),
        }),
    )
    .await;

    let listen_result = match listen_result {
        Ok(lr) => lr,
        Err(e) => {
            warn!(
                cid = %cid,
                "fold inbound failed with io::Error, {e}"
            );

            return Err(e.into());
        }
    };

    handle_in_fold_result(
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
pub async fn handle_in_fold_result(
    mut listen_result: fold::FoldResult,

    out_selector: Arc<Box<dyn OutSelector>>,

    tr: Option<Arc<net::GlobalTrafficRecorder>>,

    newc_recorder: OptNewInfoSender,

    #[cfg(feature = "trace")] updater: net::OptUpdater,
) -> anyhow::Result<()> {
    let cid = listen_result.id;

    let mut is_fallback = false;

    let target_addr = match listen_result.a.take() {
        Some(ta) => ta,
        None => {
            let return_e: anyhow::Error;
            match listen_result.e {
                Some(err) => {
                    if listen_result.c.is_some() {
                        warn!(cid = %cid, e=%err, "fold inbound failed with Error, will try to fallback: {:#}",err);

                        is_fallback = true;
                        Addr::default()
                    } else {
                        let return_e =
                            err.context("fold inbound failed with Error and can't fallback");

                        warn!(cid = %cid, "{:#}",return_e);

                        return Err(return_e);
                    }
                }
                None => match &listen_result.c {
                    Stream::None => {
                        return_e = anyhow!("fold inbound ok and stream got consumed");
                        info!(cid = %cid, "{}", return_e);
                        return Ok(());
                    }
                    _ => {
                        return_e =
                            anyhow!( "fold inbound succeed but got no target_addr, will use empty target_addr");
                        warn!(cid = %cid, "{}", return_e);
                        Addr::default()
                    }
                },
            }
        }
    };
    if !is_fallback && tracing::enabled!(tracing::Level::INFO) {
        match listen_result.b.as_ref() {
            Some(ed) => {
                info!(
                    cid = %cid,
                    "fold inbound succeed with ed, target_addr: {}, ed {}",
                    &target_addr,
                    ed.len()
                )
            }
            None => {
                info!(
                    cid = %cid,
                    target_addr = %target_addr,
                    "fold inbound succeed",
                )
            }
        }
    }

    let outbound = out_selector
        .select(
            is_fallback,
            &target_addr,
            &listen_result.chain_tag,
            &listen_result.d,
        )
        .await;

    let outbound = match outbound {
        Some(o) => o,
        None => {
            info!(cid = %cid, is_fallback = is_fallback, "out selector got None, shutting down the connection");

            let r = listen_result.c.try_shutdown().await;
            if let Err(e) = r {
                warn!(cid = %cid, e=%e, "shutdown stream got error");
            }

            return Ok(());
        }
    };

    let cid_c = cid.clone();
    let ta_clone = target_addr.clone();
    let dial_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            fold::fold(FoldParams {
                cid: cid_c,
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
            warn!(cid = %cid, is_fallback = is_fallback, "fold outbound timeout, {e}",);
            return Err(e.into());
        }
    };
    let cid = dial_result.id;

    if let Some(e) = dial_result.e {
        warn!(cid = %cid, is_fallback = is_fallback, "fold outbound failed, {:#}", e);
        return Err(e);
    }
    if let Stream::None = dial_result.c {
        warn!(
            cid = %cid, is_fallback = is_fallback,
            "fold outbound stream got consumed ",
        );

        return Ok(());
    }

    if tracing::enabled!(tracing::Level::INFO) {
        if let Some(rta) = &dial_result.a {
            if rta.eq(&Addr::default()) {
                if is_fallback {
                    info!(
                        cid = %cid,
                        "fallback to outbound succeed",
                    );
                } else {
                    info!(
                        cid = %cid, is_fallback = is_fallback,
                        "fold outbound succeed with empty target_addr",
                    );
                }
            } else {
                info!( cid = %cid, is_fallback = is_fallback,
                "fold outbound succeed, but the target_addr is not consumed, might be udp first target addr: {rta} ",);
            }
        } else {
            info!(cid = %cid, is_fallback = is_fallback,"fold outbound succeed, will start relay");
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
        dial_result.no_timeout,
        listen_result.shutdown_rx,
        dial_result.shutdown_rx,
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
    no_timeout: bool,
    shutdown_rx1: Option<tokio::sync::oneshot::Receiver<()>>,
    shutdown_rx2: Option<tokio::sync::oneshot::Receiver<()>>,

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
            tokio::spawn(cp_udp::cp_udp_tcp(
                cid,
                o,
                i,
                false,
                ed,
                first_target,
                tr,
                no_timeout,
            ));
        }
        (Stream::AddrConn(i), Stream::Conn(o)) => {
            tokio::spawn(cp_udp::cp_udp_tcp(
                cid,
                i,
                o,
                true,
                ed,
                first_target,
                tr,
                no_timeout,
            ));
        }
        (Stream::AddrConn(i), Stream::AddrConn(o)) => {
            tokio::spawn(cp_udp(
                cid,
                i,
                o,
                ed,
                first_target,
                tr,
                no_timeout,
                shutdown_rx1,
                shutdown_rx2,
            ));
        }
        (s1, s2) => {
            warn!( s1 = %s1, s2 = %s2,"can't cp stream when one of them is not (Conn or AddrConn)");
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
    no_timeout: bool,
    shutdown_rx1: Option<tokio::sync::oneshot::Receiver<()>>,
    shutdown_rx2: Option<tokio::sync::oneshot::Receiver<()>>,
) {
    use crate::Name;

    info!(cid = %cid, in_c = in_conn.name(), out_c = out_conn.name(), "relay udp start",);

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
                let _ = out_conn.w.shutdown().await;
                return;
            }
        } else {
            debug!("cp_udp: writing ed without real_first_target");
            let r = out_conn.w.write(&real_ed, &Addr::default()).await;
            if let Err(e) = r {
                warn!("cp_udp: writing ed failed: {e}");
                let _ = out_conn.w.shutdown().await;
                return;
            }
        }
    }

    let _ = net::addr_conn::cp(
        cid.clone(),
        in_conn,
        out_conn,
        tr,
        no_timeout,
        shutdown_rx1,
        shutdown_rx2,
    )
    .await;

    // debug!("cp_udp: calling shutdown");

    // let r1 = in_conn.w.shutdown().await;
    // let r2 = out_conn.w.shutdown().await;
    // debug!("cp_udp: called shutdown {:?} {:?}", r1, r2);
}
