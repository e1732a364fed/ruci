use super::*;
use log::{info, log_enabled, warn};
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;

use crate::map::*;
use crate::net;

/// 初级监听为tcp的 链式转发.
///
/// ins指 in_server, outc 指 out client;
/// ins_name 只用于 log 和 debug.
/// ins 的行为只与 ins_iterator 有关. outc 同理。
///
/// 若 outc_addr 为None 或 outc_iterator.next() 必返回 None，则说明 该 outc 为 direct直连
///
///  会在 最长 READ_HANDSHAKE_TIMEOUT 秒内 进行握手.
/// 握手后，不会阻塞，拷贝会在新的异步任务中完成
///
///
pub async fn handle_tcp<'a>(
    in_tcp: TcpStream,
    ins_name: &str,
    outc_name: &str,

    ins_iterator: impl Iterator<Item = &'a MapperBox>,
    outc_iterator: impl Iterator<Item = &'a MapperBox>,
    outc_addr: Option<net::Addr>,
    ti: Option<Arc<net::TransmissionInfo>>,
) -> io::Result<()> {
    let mut state = State::new("tcp");
    state.ins_name = ins_name.to_string();
    state.cached_in_raddr = in_tcp.peer_addr().unwrap().to_string();

    let cid = state.cid;

    //let intcpc = in_tcp.clone();

    let listen_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            type DummyType = std::vec::IntoIter<OptData>;
            //Ok::<AccumulateResult, E>(
            TcpInAccumulator::accumulate::<_, DummyType>(cid, Box::new(in_tcp), ins_iterator, None)
                .await
            //)
        })
        .await;

    let mut listen_result = match listen_result {
        Ok(lr) => lr,
        Err(e) => {
            warn!(
                "{}, handshake in server failed with io::Error, {}",
                state, e
            );
            //let _ = in_tcp.shutdown();

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
            //let _ = in_tcp.shutdown();
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
    let socket_addr = match real_target_addr.get_socket_addr_or_resolve() {
        Ok(t) => t,
        Err(e) => {
            warn!(
                "{}, parse target addr failed, {} , {}",
                state, real_target_addr, e
            );
            //let _ = in_tcp.shutdown();
            return Err(e);
        }
    };

    let out_tcp = match TcpStream::connect(socket_addr).await {
        Ok(tcp) => tcp,
        Err(e) => {
            warn!("{}, handshake in server failed: {}", state, e);
            //let _ = in_tcp.shutdown();
            return Err(e);
        }
    };
    let out_conn = Box::new(out_tcp);

    if is_direct {
        cp_tcp::cp_tcp(
            cid,
            // in_tcp,
            // out_tcp,
            listen_result.c.take().unwrap(),
            out_conn,
            listen_result.b.take(),
            ti,
        )
        .await;
    } else {
        let dial_result =
            tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
                TcpOutAccumulator::accumulate(
                    cid,
                    out_conn,
                    outc_iterator,
                    Some(target_addr),
                    listen_result.b.take(),
                )
                .await
            })
            .await;

        if let Err(e) = dial_result {
            warn!("{}, dial out client timeout, {}", state, e);
            //let _ = in_tcp.shutdown();
            //let _ = out_tcp.shutdown();
            return Err(e.into());
        }
        let dial_result = dial_result.unwrap();
        if let Err(e) = dial_result {
            warn!("{}, dial out client failed, {}", state, e);
            //let _ = in_tcp.shutdown();
            //let _ = out_tcp.shutdown();
            return Err(e);
        }

        let (out_conn, remain_target_addr, _extra_out_data_vec) = dial_result.unwrap();
        if let Some(rta) = remain_target_addr {
            warn!(
                "{}, dial out client succeed, but the target_addr is not consumed, {} ",
                state, rta
            );
        }
        cp_tcp::cp_tcp(
            cid,
            //in_tcp,
            //out_tcp,
            listen_result.c.take().unwrap(),
            out_conn,
            None,
            ti,
        )
        .await;
    }

    Ok(())
}
