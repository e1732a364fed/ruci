use super::*;
use bytes::BytesMut;

use log::{info, log_enabled, warn};
use std::io;
use std::sync::Arc;
use std::time::Duration;

use crate::map;
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
    let mut state = RootState::new(network_str);
    state.set_ins_name(ins_name.to_string());
    state.set_cached_in_raddr(in_raddr);

    let cid = state.cid();

    type ExtraDataIterType = std::vec::IntoIter<OptData>;

    let listen_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            map::accumulate::<_, ExtraDataIterType>(
                cid,
                ProxyBehavior::DECODE,
                MapResult::c(in_conn),
                ins_iterator,
                None,
            )
            .await
        })
        .await;

    let mut listen_result = match listen_result {
        Ok(lr) => lr,
        Err(e) => {
            warn!("{state}, handshake in server failed with io::Error, {e}");

            return Err(e.into());
        }
    };

    let target_addr = match listen_result.a.take() {
        Some(ta) => ta,
        None => {
            warn!("{state}, handshake in server succeed but got no target_addr",);
            let _ = listen_result.c.try_shutdown().await;
            return Err(io::Error::other(
                "handshake in server succeed but got no target_addr",
            ));
        }
    };

    if log_enabled!(log::Level::Info) {
        info!(
            "{state}, handshake in server succeed, target_addr: {}",
            &target_addr
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
    state.set_outc_name(outc_name.to_string());

    //todo: DNS 功能

    let out_stream = match real_target_addr.try_dial().await {
        Ok(t) => t,
        Err(e) => {
            warn!("{state}, parse target addr failed, {real_target_addr} , {e}",);
            let _ = listen_result.c.try_shutdown().await;
            return Err(e);
        }
    };

    if is_direct {
        cp_stream(cid, listen_result.c, out_stream, listen_result.b.take(), ti).await;
    } else {
        let dial_result =
            tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
                map::accumulate::<_, ExtraDataIterType>(
                    cid,
                    ProxyBehavior::ENCODE,
                    MapResult {
                        a: Some(target_addr),
                        b: listen_result.b.take(),
                        c: out_stream,
                        d: None,
                        e: None,
                    },
                    outc_iterator,
                    None,
                )
                .await
            })
            .await;

        if let Err(e) = dial_result {
            warn!("{state}, dial out client timeout, {e}",);
            //let _ = in_conn.shutdown();
            //let _ = out_stream.try_shutdown();
            return Err(e.into());
        }
        let dial_result = dial_result.unwrap();
        if let Some(e) = dial_result.e {
            warn!("{state}, dial out client failed, {e}",);
            //let _ = in_conn.shutdown();
            //let _ = out_stream.try_shutdown();
            return Err(e);
        } else if let Stream::None = dial_result.c {
            info!("{state}, dial out client stream got consumed ",);

            return Ok(());
        }

        //let (out_stream, remain_target_addr, _extra_out_data_vec) = dial_result;
        if let Some(rta) = dial_result.a {
            warn!("{state}, dial out client succeed, but the target_addr is not consumed, {rta} ",);
        }
        cp_stream(cid, listen_result.c, dial_result.c, None, ti).await;
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
    info!("cid: {cid}, relay udp start",);

    //discard early data, as we don't know it's target addr

    let tic = ti.clone();
    scopeguard::defer! {

        if let Some(ti) = tic {
            ti.alive_connection_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        }
        info!("cid: {cid},udp relay end" );
    }

    let _ = net::addr_conn::cp(cid, in_conn, out_conn, ti).await;
}
