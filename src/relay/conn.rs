use super::*;
use bytes::BytesMut;

use log::{info, log_enabled, warn};
use std::io;
use std::sync::Arc;
use std::time::Duration;

use crate::map::*;
use crate::net;

use crate::net::Stream;

/// 初级监听为 net::Conn 的 链式转发.
///
/// ins指 in_server, outc 指 out client;
/// ins_name 只用于 log 和 debug.
/// ins 的行为只与 ins_iterator 有关, 与name无关. outc 同理。
///
/// 若 outc_addr 为None 或 outc_iterator.next() 必返回 None，则说明 该 outc 为 direct直连
///
///  会在 最长 READ_HANDSHAKE_TIMEOUT 秒内 进行握手.
/// 握手后，不会阻塞，拷贝会在新的异步任务中完成
///
///
pub async fn handle_conn<'a>(
    in_conn: net::Conn,
    ins_name: &str,
    outc_name: &str,
    in_raddr: String,
    network_str: &'static str,

    ins_iterator: impl Iterator<Item = &'a MapperBox>,
    outc_iterator: impl Iterator<Item = &'a MapperBox>,
    outc_addr: Option<net::Addr>,
    ti: Option<Arc<net::TransmissionInfo>>,
) -> io::Result<()> {
    let mut state = State::new(network_str);
    state.ins_name = ins_name.to_string();
    state.cached_in_raddr = in_raddr;

    let cid = state.cid;

    let listen_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            type DummyType = std::vec::IntoIter<OptData>;
            TcpInAccumulator::accumulate::<_, DummyType>(cid, in_conn, ins_iterator, None).await
        })
        .await;

    let mut listen_result = match listen_result {
        Ok(lr) => lr,
        Err(e) => {
            warn!(
                "{}, handshake in server failed with io::Error, {}",
                state, e
            );
            //let _ = in_conn.shutdown();

            return Err(e.into());
        }
    };

    let target_addr = match listen_result.a.take() {
        Some(ta) => ta,
        None => {
            warn!(
                "{}, handshake in server succeed but got no target_addr",
                state
            );
            if let Some(c) = listen_result.c {
                let _ = c.try_shutdown().await;
            }
            return Err(io::Error::other(
                "handshake in server succeed but got no target_addr",
            ));
        }
    };

    if log_enabled!(log::Level::Info) {
        info!(
            "{}, handshake in server succeed, target_addr: {}",
            state, &target_addr
        )
    }

    let is_direct: bool;

    //todo: 路由功能
    let real_target_addr = if outc_addr.is_some() {
        is_direct = false;
        outc_addr.unwrap()
    } else {
        is_direct = true;
        target_addr.clone()
    };
    state.outc_name = outc_name.to_string();

    //todo: DNS 功能

    let out_stream = match real_target_addr.try_dial().await {
        Ok(t) => t,
        Err(e) => {
            warn!(
                "{}, parse target addr failed, {} , {}",
                state, real_target_addr, e
            );
            if let Some(c) = listen_result.c {
                let _ = c.try_shutdown().await;
            }
            return Err(e);
        }
    };

    if is_direct {
        cp_stream(
            cid,
            listen_result.c.take().unwrap(),
            out_stream,
            listen_result.b.take(),
            ti,
        )
        .await;
    } else {
        let dial_result =
            tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
                TcpOutAccumulator::accumulate(
                    cid,
                    out_stream,
                    outc_iterator,
                    Some(target_addr),
                    listen_result.b.take(),
                )
                .await
            })
            .await;

        if let Err(e) = dial_result {
            warn!("{}, dial out client timeout, {}", state, e);
            //let _ = in_conn.shutdown();
            //let _ = out_stream.try_shutdown();
            return Err(e.into());
        }
        let dial_result = dial_result.unwrap();
        if let Err(e) = dial_result {
            warn!("{}, dial out client failed, {}", state, e);
            //let _ = in_conn.shutdown();
            //let _ = out_stream.try_shutdown();
            return Err(e);
        }

        let (out_stream, remain_target_addr, _extra_out_data_vec) = dial_result.unwrap();
        if let Some(rta) = remain_target_addr {
            warn!(
                "{}, dial out client succeed, but the target_addr is not consumed, {} ",
                state, rta
            );
        }
        cp_stream(cid, listen_result.c.take().unwrap(), out_stream, None, ti).await;
    }

    Ok(())
}

pub async fn cp_stream(
    cid: u32,
    s1: Stream,
    s2: Stream,
    ed: Option<BytesMut>, //earlydata
    ti: Option<Arc<net::TransmissionInfo>>,
) {
    match (s1, s2) {
        (Stream::TCP(i), Stream::TCP(o)) => cp_tcp::cp_conn(cid, i, o, ed, ti).await,
        (Stream::TCP(i), Stream::UDP(o)) => {
            let _ = cp_udp::cp_udp_tcp(cid, o, i, false, ed, ti).await;
        }
        (Stream::UDP(i), Stream::TCP(o)) => {
            let _ = cp_udp::cp_udp_tcp(cid, i, o, true, ed, ti).await;
        }
        (Stream::UDP(i), Stream::UDP(o)) => cp_udp(cid, i, o, ti).await,
        _ => {
            warn!("can't cp stream when either of them is None");
        }
    }
}

pub async fn cp_udp(
    cid: u32,
    in_conn: net::addr_conn::AddrConn,
    out_conn: net::addr_conn::AddrConn,
    ti: Option<Arc<net::TransmissionInfo>>,
) {
    info!("cid: {}, relay udp start", cid);

    //discard early data, as we don't know it's target addr

    let tic = ti.clone();
    scopeguard::defer! {

        if let Some(ti) = tic {
            ti.alive_connection_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        }
        info!("cid: {},udp relay end", cid);
    }

    let _ = net::addr_conn::cp(cid, in_conn, out_conn, ti).await;
}
