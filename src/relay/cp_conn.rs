/*!
cp_conn 包含4种情况, 对应有无earlydata 和 有无 [`Arc<GlobalTrafficRecorder>`]

*/

use crate::net::CID;
use crate::net::{self, GlobalTrafficRecorder};
use scopeguard::defer;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tracing::{debug, warn};

/// copy between two [`net::Conn`]
///
/// non-blocking, spawns new task to do actual relay
pub fn cp_conn(
    cid: CID,
    in_conn: net::Conn,
    out_conn: net::Conn,
    pre_read_data: Option<bytes::BytesMut>,
    gtr: Option<Arc<net::GlobalTrafficRecorder>>,

    #[cfg(feature = "trace")] updater: net::OptUpdater,
) {
    match (pre_read_data, gtr) {
        (None, None) => tokio::spawn(no_gtr_no_ed(cid, in_conn, out_conn)),
        (None, Some(t)) => tokio::spawn(gtr_no_ed(
            cid,
            in_conn,
            out_conn,
            t,
            #[cfg(feature = "trace")]
            updater,
        )),
        (Some(ed), None) => tokio::spawn(no_gtr_ed(cid, in_conn, out_conn, ed)),
        (Some(ed), Some(t)) => tokio::spawn(gtr_ed(
            cid,
            in_conn,
            out_conn,
            t,
            ed,
            #[cfg(feature = "trace")]
            updater,
        )),
    };
}

async fn no_gtr_no_ed(cid: CID, mut in_conn: net::Conn, mut out_conn: net::Conn) {
    debug!(cid = %cid, "relay start");

    let r = net::copy(
        &mut in_conn,
        &mut out_conn,
        &cid,
        #[cfg(feature = "trace")]
        None,
    )
    .await;

    match r {
        Ok(_) =>{
            
        },
        Err(e) => tracing::info!(cid = %cid, err = %e, "relay got error"),
    }

    debug!(cid = %cid, "relay end",);
}

async fn gtr_no_ed(
    cid: CID,
    mut in_conn: net::Conn,
    mut out_conn: net::Conn,
    gtr: Arc<GlobalTrafficRecorder>,

    #[cfg(feature = "trace")] updater: net::OptUpdater,
) {
    debug!(cid = %cid, "relay start");

    gtr.alive_connection_count.fetch_add(1, Ordering::Relaxed);

    defer! {
        gtr.alive_connection_count.fetch_sub(1, Ordering::Relaxed);
        debug!(cid = %cid, "relay end", );
    }

    let r = net::copy(
        &mut in_conn,
        &mut out_conn,
        &cid,
        #[cfg(feature = "trace")]
        updater,
    )
    .await;

    match r {
        Ok((u, d)) =>{
            gtr.ub.fetch_add(u, Ordering::Relaxed);
            gtr.db.fetch_add(d, Ordering::Relaxed);
        },
        Err(e) => tracing::info!(cid = %cid, err = %e, "relay got error"),
    }
    
}

async fn no_gtr_ed(
    cid: CID,
    mut in_conn: net::Conn,
    mut out_conn: net::Conn,
    earlydata: bytes::BytesMut,
) {
    if tracing::enabled!(tracing::Level::DEBUG) {
        debug!(
            cid = %cid,
            "relay with earlydata, {}",
            earlydata.len()
        );
    }
    let r = out_conn.write_all(&earlydata).await;
    match r {
        Ok(_) => {
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!(cid = %cid, "upload earlydata ok, ");
            }
            let r = out_conn.flush().await;
            if let Err(e) = r {
                warn!(
                    cid = %cid,
                    "upload early_data flush failed: {}", e
                );
                let _ = in_conn.shutdown().await;
                let _ = out_conn.shutdown().await;
                return;
            }
        }
        Err(e) => {
            warn!(cid = %cid, "upload early_data failed: {}", e);

            let _ = in_conn.shutdown().await;
            let _ = out_conn.shutdown().await;
            return;
        }
    }

    let r = net::copy(
        &mut in_conn,
        &mut out_conn,
        &cid,
        #[cfg(feature = "trace")]
        None,
    )
    .await;

    match r {
        Ok(_) =>{
            
        },
        Err(e) => tracing::info!(cid = %cid, err = %e, "relay got error"),
    }

    debug!(cid = %cid, "relay end");
}

async fn gtr_ed(
    cid: CID,
    mut in_conn: net::Conn,
    mut out_conn: net::Conn,
    gtr: Arc<GlobalTrafficRecorder>,
    earlydata: bytes::BytesMut,

    #[cfg(feature = "trace")] updater: net::OptUpdater,
) {
    if tracing::enabled!(tracing::Level::DEBUG) {
        debug!(
            cid = %cid,
            "relay with earlydata, {}",
            earlydata.len()
        );
    }
    let r = out_conn.write_all(&earlydata).await;
    match r {
        Ok(_) => {
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!(cid = %cid, "upload earlydata ok {}",earlydata.len());
            }

            gtr.ub.fetch_add(earlydata.len() as u64, Ordering::Relaxed);
        }
        Err(e) => {
            warn!(cid = %cid, "upload early_data failed: {}", e);

            let _ = in_conn.shutdown().await;
            let _ = out_conn.shutdown().await;
            return;
        }
    }

    gtr.alive_connection_count.fetch_add(1, Ordering::Relaxed);

    defer! {
        gtr.alive_connection_count.fetch_sub(1, Ordering::Relaxed);
        debug!(cid = %cid, "relay end");
    }

    let r = net::copy(
        &mut in_conn,
        &mut out_conn,
        &cid,
        #[cfg(feature = "trace")]
        updater,
    )
    .await;

    match r {
        Ok((u, d)) =>{
            gtr.ub.fetch_add(u, Ordering::Relaxed);
            gtr.db.fetch_add(d, Ordering::Relaxed);
        },
        Err(e) => tracing::info!(cid = %cid, err = %e, "relay got error"),
    }
}
