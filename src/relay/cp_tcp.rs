/*!
 * cp_conn 包含4种情况, 对应有无earlydata 和 有无 Arc<GlobalTrafficRecorder>
 *
 * （这是将两个条件判断转成状态机的做法）
*/

use crate::net::CID;
use crate::net::{self, GlobalTrafficRecorder};
use log::Level::Debug;
use log::{debug, log_enabled, warn};
use scopeguard::defer;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

//non-blocking, spawns new task to do actual relay
pub fn cp_conn(
    cid: CID,
    in_conn: net::Conn,
    out_conn: net::Conn,
    pre_read_data: Option<bytes::BytesMut>,
    ti: Option<Arc<net::GlobalTrafficRecorder>>,

    #[cfg(feature = "trace")] updater: net::OptUpdater,
) {
    match (pre_read_data, ti) {
        (None, None) => tokio::spawn(no_ti_no_ed(cid, in_conn, out_conn)),
        (None, Some(t)) => tokio::spawn(ti_no_ed(
            cid,
            in_conn,
            out_conn,
            t,
            #[cfg(feature = "trace")]
            updater,
        )),
        (Some(ed), None) => tokio::spawn(no_ti_ed(cid, in_conn, out_conn, ed)),
        (Some(ed), Some(t)) => tokio::spawn(ti_ed(
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

async fn no_ti_no_ed(cid: CID, in_conn: net::Conn, out_conn: net::Conn) {
    debug!("{cid}, relay start");

    let _ = net::copy(
        in_conn,
        out_conn,
        &cid,
        None,
        #[cfg(feature = "trace")]
        None,
    )
    .await;
    debug!("{cid}, relay end",);
}

async fn ti_no_ed(
    cid: CID,
    in_conn: net::Conn,
    out_conn: net::Conn,
    ti: Arc<GlobalTrafficRecorder>,

    #[cfg(feature = "trace")] updater: net::OptUpdater,
) {
    debug!("{cid}, relay start");

    ti.alive_connection_count.fetch_add(1, Ordering::Relaxed);

    defer! {
        ti.alive_connection_count.fetch_sub(1, Ordering::Relaxed);
        debug!("{cid}, relay end", );
    }

    let _ = net::copy(
        in_conn,
        out_conn,
        &cid,
        Some(ti.clone()),
        #[cfg(feature = "trace")]
        updater,
    )
    .await;
}

async fn no_ti_ed(
    cid: CID,
    mut in_conn: net::Conn,
    mut out_conn: net::Conn,
    earlydata: bytes::BytesMut,
) {
    if log_enabled!(Debug) {
        debug!("{cid}, relay with earlydata, {}", earlydata.len());
    }
    let r = out_conn.write_all(&earlydata).await;
    match r {
        Ok(_) => {
            if log_enabled!(Debug) {
                debug!("{cid}, upload earlydata ok, ");
            }
            let r = out_conn.flush().await;
            if let Err(e) = r {
                warn!("{cid}, upload early_data flush failed: {}", e);
                let _ = in_conn.shutdown().await;
                let _ = out_conn.shutdown().await;
                return;
            }
        }
        Err(e) => {
            warn!("{cid}, upload early_data failed: {}", e);

            let _ = in_conn.shutdown().await;
            let _ = out_conn.shutdown().await;
            return;
        }
    }

    let _ = net::copy(
        in_conn,
        out_conn,
        &cid,
        None,
        #[cfg(feature = "trace")]
        None,
    )
    .await;

    debug!("{}, relay end", cid);
}

async fn ti_ed(
    cid: CID,
    mut in_conn: net::Conn,
    mut out_conn: net::Conn,
    ti: Arc<GlobalTrafficRecorder>,
    earlydata: bytes::BytesMut,

    #[cfg(feature = "trace")] updater: net::OptUpdater,
) {
    if log_enabled!(Debug) {
        debug!("{cid}, relay with earlydata, {}", earlydata.len());
    }
    let r = out_conn.write_all(&earlydata).await;
    match r {
        Ok(_) => {
            if log_enabled!(Debug) {
                debug!("{cid}, upload earlydata ok ");
            }

            ti.ub.fetch_add(earlydata.len() as u64, Ordering::Relaxed);
        }
        Err(e) => {
            warn!("{cid}, upload early_data failed: {}", e);

            let _ = in_conn.shutdown().await;
            let _ = out_conn.shutdown().await;
            return;
        }
    }

    ti.alive_connection_count.fetch_add(1, Ordering::Relaxed);

    defer! {
        ti.alive_connection_count.fetch_sub(1, Ordering::Relaxed);
        debug!("{cid}, relay end");
    }

    let _ = net::copy(
        in_conn,
        out_conn,
        &cid,
        Some(ti.clone()),
        #[cfg(feature = "trace")]
        updater,
    )
    .await;
}
