/*!
 * relay 包定义了一种转发逻辑，但是它不是强制性的，可用于参考。
具体实现 中可以有不同的转发逻辑

*/
pub mod conn;
pub mod cp_tcp;
pub mod cp_udp;
pub mod route;

use std::sync::Arc;

use bytes::BytesMut;
use log::{info, warn};

use crate::net::{self, Stream, CID};

pub const READ_HANDSHAKE_TIMEOUT: u64 = 15; // 15秒的最长握手等待时间。 //todo: 修改这里

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
