/*!
 * cp_tcp 包含4种情况，对应有无earlydata 和 有无 Arc<TransmissionInfo>
 *
 * （这是将两个条件判断转成状态机的做法）
*/

use crate::net::{self, TransmissionInfo};
use log::Level::Debug;
use log::{debug, info, log_enabled, warn};
use std::sync::atomic::Ordering;
use std::sync::Arc;

//non-blocking
pub async fn cp_tcp(
    cid: u32,
    raw_intcp: TcpStream,
    raw_out_tcp: TcpStream,
    in_conn: net::Conn,
    out_conn: net::Conn,
    pre_read_data: Option<bytes::BytesMut>,
    ti: Option<Arc<net::TransmissionInfo>>,
) {
    let cf = move || {
        let _ = raw_intcp.shutdown(std::net::Shutdown::Both);
        let _ = raw_out_tcp.shutdown(std::net::Shutdown::Both);
    };

    info!("cid: {}, relay start", cid);

    match (pre_read_data, ti) {
        (None, None) => task::spawn(no_ti_no_ed(cid, in_conn, out_conn, cf)),
        (None, Some(t)) => task::spawn(ti_no_ed(cid, in_conn, out_conn, cf, t)),
        (Some(ed), None) => task::spawn(no_ti_ed(cid, in_conn, out_conn, cf, ed)),
        (Some(ed), Some(t)) => task::spawn(ti_ed(cid, in_conn, out_conn, cf, t, ed)),
    };
}

async fn no_ti_no_ed(cid: u32, in_conn: net::Conn, out_tcp: net::Conn, cf: impl Fn()) {
    let _ = net::cp(in_conn, out_tcp, cid, cf, None).await;
    info!("cid: {}, relay end", cid);
}

async fn ti_no_ed(
    cid: u32,
    in_conn: net::Conn,
    out_conn: net::Conn,
    cf: impl Fn(),
    ti: Arc<TransmissionInfo>,
) {
    ti.alive_connection_count.fetch_add(1, Ordering::Relaxed);
    let _ = net::cp(in_conn, out_conn, cid, cf, Some(ti.clone())).await;
    ti.alive_connection_count.fetch_sub(1, Ordering::Relaxed);
    info!("cid: {}, relay end", cid);
}

async fn no_ti_ed(
    cid: u32,
    in_conn: net::Conn,
    mut out_conn: net::Conn,
    cf: impl Fn(),
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

            cf();
            return;
        }
    }

    let _ = net::cp(in_conn, out_conn, cid, cf, None).await;

    info!("cid: {}, relay end", cid);
}

async fn ti_ed(
    cid: u32,
    in_conn: net::Conn,
    mut out_conn: net::Conn,
    cf: impl Fn(),
    ti: Arc<TransmissionInfo>,
    earlydata: bytes::BytesMut,
) {
    ti.alive_connection_count.fetch_add(1, Ordering::Relaxed);

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

            cf();
            return;
        }
    }

    let _ = net::cp(in_conn, out_conn, cid, cf, Some(ti.clone())).await;
    ti.alive_connection_count.fetch_sub(1, Ordering::Relaxed);

    info!("cid: {}, relay end", cid);
}
