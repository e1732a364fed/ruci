/*!
 * cp_tcp 包含4种情况，对应有无earlydata 和 有无 Arc<TransmissionInfo>
 *
 * （这是将两个条件判断转成状态机的做法）
*/

use crate::net::CID;
use crate::net::{self, TransmissionInfo};
use log::Level::Debug;
use log::{debug, info, log_enabled, warn};
use scopeguard::defer;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::task;

//non-blocking
pub async fn cp_conn(
    cid: CID,
    in_conn: net::Conn,
    out_conn: net::Conn,
    pre_read_data: Option<bytes::BytesMut>,
    ti: Option<Arc<net::TransmissionInfo>>,
) {
    info!("cid: {}, relay start", cid);

    match (pre_read_data, ti) {
        (None, None) => task::spawn(no_ti_no_ed(cid, in_conn, out_conn)),
        (None, Some(t)) => task::spawn(ti_no_ed(cid, in_conn, out_conn, t)),
        (Some(ed), None) => task::spawn(no_ti_ed(cid, in_conn, out_conn, ed)),
        (Some(ed), Some(t)) => task::spawn(ti_ed(cid, in_conn, out_conn, t, ed)),
    };
}

async fn no_ti_no_ed(cid: CID, in_conn: net::Conn, out_conn: net::Conn) {
    let _ = net::cp(in_conn, out_conn, &cid, None).await;
    info!("cid: {}, relay end", cid);
}

async fn ti_no_ed(cid: CID, in_conn: net::Conn, out_conn: net::Conn, ti: Arc<TransmissionInfo>) {
    ti.alive_connection_count.fetch_add(1, Ordering::Relaxed);

    defer! {
        ti.alive_connection_count.fetch_sub(1, Ordering::Relaxed);
        info!("cid: {}, relay end", cid);
    }

    let _ = net::cp(in_conn, out_conn, &cid, Some(ti.clone())).await;
}

async fn no_ti_ed(
    cid: CID,
    mut in_conn: net::Conn,
    mut out_conn: net::Conn,
    earlydata: bytes::BytesMut,
) {
    if log_enabled!(Debug) {
        debug!("cid: {}, relay with earlydata, {}", cid, earlydata.len());
    }
    let r = out_conn.write(&earlydata).await;
    match r {
        Ok(upload_bytes) => {
            if log_enabled!(Debug) {
                debug!("cid: {}, upload earlydata ok, {}", cid, upload_bytes);
            }
        }
        Err(e) => {
            warn!("cid: {}, upload early_data failed: {}", cid, e);

            let _ = in_conn.shutdown().await;
            let _ = out_conn.shutdown().await;
            return;
        }
    }

    let _ = net::cp(in_conn, out_conn, &cid, None).await;

    info!("cid: {}, relay end", cid);
}

async fn ti_ed(
    cid: CID,
    mut in_conn: net::Conn,
    mut out_conn: net::Conn,
    ti: Arc<TransmissionInfo>,
    earlydata: bytes::BytesMut,
) {
    if log_enabled!(Debug) {
        debug!("cid: {}, relay with earlydata, {}", cid, earlydata.len());
    }
    let r = out_conn.write(&earlydata).await;
    match r {
        Ok(upload_bytes) => {
            if log_enabled!(Debug) {
                debug!("cid: {}, upload earlydata ok, {}", cid, upload_bytes);
            }

            ti.ub.fetch_add(upload_bytes as u64, Ordering::Relaxed);
        }
        Err(e) => {
            warn!("cid: {}, upload early_data failed: {}", cid, e);

            let _ = in_conn.shutdown().await;
            let _ = out_conn.shutdown().await;
            return;
        }
    }

    ti.alive_connection_count.fetch_add(1, Ordering::Relaxed);

    defer! {
        ti.alive_connection_count.fetch_sub(1, Ordering::Relaxed);
        info!("cid: {}, relay end", cid);
    }

    let _ = net::cp(in_conn, out_conn, &cid, Some(ti.clone())).await;

    info!("cid: {}, relay end", cid);
}
