/*!
relay 包定义了一种转发逻辑, 但是它不是强制性的, 可用于参考.
具体实现 中可以有不同的转发逻辑

*/
mod cp_ac_conn;
mod cp_conn;

pub use cp_ac_conn::*;
pub use cp_conn::*;

pub mod record;
pub mod route;

pub use record::*;

use std::sync::Arc;

use bytes::BytesMut;
use tracing::{debug, info, warn};

use crate::net::addr_conn::{AddrConn, AsyncWriteAddrExt};
use crate::net::{self, Addr, Stream, CID};

use self::fold::{DMIterBox, FoldParams};
use self::route::OutSelector;

use anyhow::anyhow;
use std::time::Duration;

use crate::map::*;

pub const READ_HANDSHAKE_TIMEOUT: u64 = 15; // 15秒的最长握手等待时间.  //todo: adjust this

/// this function utilizes [`handle_in_fold_result`] and  [`OutSelector`]
/// to select an outbound, fold it and then copy streams.
///
/// block until in and out handshake is over.
///
pub async fn handle_in_stream(
    in_conn: Stream,
    ins_iterator: DMIterBox,
    out_selector: Arc<Box<dyn OutSelector>>,
    gtr: Option<Arc<net::GlobalTrafficRecorder>>,

    newc_recorder: OptNewInfoSender,

    #[cfg(feature = "trace")] updater: net::OptUpdater,
) -> anyhow::Result<()> {
    let cid = match gtr.as_ref() {
        Some(gtr) => CID::new_ordered(&gtr.last_connection_id),
        None => CID::new_random(),
    };

    let cid_c = cid.clone();
    let listen_result = tokio::time::timeout(
        Duration::from_secs(READ_HANDSHAKE_TIMEOUT),
        fold::fold(FoldParams {
            cid: cid_c,
            behavior: ProxyBehavior::DECODE,
            initial_state: MapResult::builder().c(in_conn).build(),
            maps: ins_iterator,
            chain_tag: String::new(),
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

/// fold the inbound, select an outbound, fold the outbound, then calls
/// [`cp_stream`] to copy between the inbound stream and outbound stream.
///
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
    debug!(cid = %cid, in_tag = listen_result.chain_tag, target_addr = %target_addr, "try select out",);

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
                maps: outbound,
                chain_tag: String::new(),
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

    cp_stream(CpStreamArgs {
        cid,
        in_stream: listen_result.c,
        out_stream: dial_result.c,
        ed: dial_result.b,
        first_target: dial_result.a,
        tr,
        no_timeout: dial_result.no_timeout,
        shutdown_in_rx: listen_result.shutdown_rx,
        shutdown_out_rx: dial_result.shutdown_rx,
        #[cfg(feature = "trace")]
        updater,
    })
    .await;

    Ok(())
}

pub struct CpStreamArgs {
    pub cid: CID,
    pub in_stream: Stream,
    pub out_stream: Stream,
    pub ed: Option<BytesMut>,            //earlydata
    pub first_target: Option<net::Addr>, // 用于 udp
    pub tr: Option<Arc<net::GlobalTrafficRecorder>>,
    pub no_timeout: bool,
    pub shutdown_in_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    pub shutdown_out_rx: Option<tokio::sync::oneshot::Receiver<()>>,

    #[cfg(feature = "trace")]
    pub updater: net::OptUpdater,
}

/// copy between two [`Stream`]
///
/// non-blocking,
pub async fn cp_stream(args: CpStreamArgs) {
    let cid = args.cid;
    let s1 = args.in_stream;
    let s2 = args.out_stream;
    let ed = args.ed;
    let first_target = args.first_target;
    let tr = args.tr;
    let shutdown_in_rx = args.shutdown_in_rx;
    let shutdown_out_rx = args.shutdown_out_rx;
    let no_timeout = args.no_timeout;

    //todo 原计划是 add trace for udp, 但因为 trace 功能用得少, 就先搁置.

    match (s1, s2) {
        (Stream::Conn(i), Stream::Conn(o)) => cp_conn::cp_conn(
            cid,
            i,
            o,
            ed,
            tr,
            #[cfg(feature = "trace")]
            args.updater,
        ),
        (Stream::Conn(i), Stream::AddrConn(o)) => {
            tokio::spawn(cp_ac_conn::cp_addr_conn_and_conn(
                cp_ac_conn::CpAddrConnAndConnArgs {
                    cid,
                    ac: o,
                    c: i,
                    ed_from_ac: false,
                    ed,
                    first_target,
                    gtr: tr,
                    no_timeout,
                    shutdown_ac_rx: shutdown_out_rx,
                },
            ));
        }
        (Stream::AddrConn(i), Stream::Conn(o)) => {
            tokio::spawn(cp_ac_conn::cp_addr_conn_and_conn(
                cp_ac_conn::CpAddrConnAndConnArgs {
                    cid,
                    ac: i,
                    c: o,
                    ed_from_ac: true,
                    ed,
                    first_target,
                    gtr: tr,
                    no_timeout,
                    shutdown_ac_rx: shutdown_in_rx,
                },
            ));
        }
        (Stream::AddrConn(i), Stream::AddrConn(o)) => {
            cp_addr_conn(CpAddrConnArgs {
                cid,
                in_conn: i,
                out_conn: o,
                ed,
                first_target,
                tr,
                no_timeout,
                shutdown_in_rx,
                shutdown_out_rx,
            })
            .await;
        }
        (s1, s2) => {
            warn!( s1 = %s1, s2 = %s2,"can't cp stream when one of them is not (Conn or AddrConn)");
        }
    }
}

pub struct CpAddrConnArgs {
    cid: CID,
    in_conn: AddrConn,
    out_conn: AddrConn,
    ed: Option<BytesMut>,
    first_target: Option<net::Addr>,
    tr: Option<Arc<net::GlobalTrafficRecorder>>,
    no_timeout: bool,
    shutdown_in_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    shutdown_out_rx: Option<tokio::sync::oneshot::Receiver<()>>,
}

/// copy between two [`AddrConn`]
///
/// non-blocking
///
pub async fn cp_addr_conn(args: CpAddrConnArgs) {
    use crate::Name;

    let cid = args.cid;
    let in_conn = args.in_conn;
    let mut out_conn = args.out_conn;
    let ed = args.ed;
    let first_target = args.first_target;
    let tr = args.tr;
    let no_timeout = args.no_timeout;
    let shutdown_in_rx = args.shutdown_in_rx;
    let shutdown_out_rx = args.shutdown_out_rx;

    info!(cid = %cid, in_c = in_conn.name(), out_c = out_conn.name(), "cp_addr_conn start",);

    if let Some(real_ed) = ed {
        if let Some(real_first_target) = first_target {
            debug!("cp_addr_conn: writing ed {:?}", real_ed.len());
            let r = out_conn.w.write(&real_ed, &real_first_target).await;
            if let Err(e) = r {
                warn!("cp_addr_conn: writing ed failed: {e}");
                let _ = out_conn.w.shutdown().await;
                return;
            }
        } else {
            debug!(
                "cp_addr_conn: writing ed without real_first_target {:?}",
                real_ed.len()
            );
            let r = out_conn.w.write(&real_ed, &Addr::default()).await;
            if let Err(e) = r {
                warn!("cp_addr_conn: writing ed failed: {e}");
                let _ = out_conn.w.shutdown().await;
                return;
            }
        }
    }

    tokio::spawn(net::addr_conn::cp(
        cid.clone(),
        in_conn,
        out_conn,
        tr,
        no_timeout,
        shutdown_in_rx,
        shutdown_out_rx,
    ));
}
