use std::{fmt::Display, time::Duration};

use super::*;
use log::warn;
use ruci::{
    map::acc2::MIterBox,
    net::{Stream, CID},
    relay::{cp_stream, READ_HANDSHAKE_TIMEOUT},
};

//todo: 移除 State。其作用不大

/// 描述一条代理连接
pub trait State<T>
where
    T: Display,
{
    fn cid(&self) -> CID;
    fn network(&self) -> &'static str;
    fn ins_name(&self) -> String; // 入口名称
    fn set_ins_name(&mut self, str: String);
    fn outc_name(&self) -> String; // 出口名称
    fn set_outc_name(&mut self, str: String);

    fn cached_in_raddr(&self) -> String; // 进入程序时的 连接 的远端地址
    fn set_cached_in_raddr(&mut self, str: String);
}

/// 阻塞监听 ins。
///
/// 确保调用 listen_ser 前, ins 和 outc 的
/// generate_upper_mappers 方法被调用过了
pub async fn listen_ser(
    ins: &'static dyn Suit,
    outc: &'static dyn Suit,
    oti: Option<Arc<net::TransmissionInfo>>,
    shutdown_rx: oneshot::Receiver<()>,
) -> io::Result<()> {
    let n = ins.network();
    match n {
        "tcp" => {
            if outc.network() != "tcp" {
                panic!(
                    "not implemented for dialing network other than tcp: {}",
                    outc.network()
                )
            }
            listen_tcp(ins, outc, oti, shutdown_rx).await
        }
        _ => Err(io::Error::other(format!(
            "such network not supported: {}",
            n
        ))),
    }
}

/// 阻塞监听 ins tcp。
async fn listen_tcp(
    ins: &'static dyn Suit,
    outc: &'static dyn Suit,
    oti: Option<Arc<net::TransmissionInfo>>,
    shutdown_rx: oneshot::Receiver<()>,
) -> io::Result<()> {
    let laddr = ins.addr_str().to_string();
    let wn = ins.whole_name().to_string();
    info!("start listen tcp {}, {}", laddr, wn);

    let listener = TcpListener::bind(laddr.clone()).await?;

    let clone_oti = move || oti.clone();

    // let ins = Box::new(ins);
    // let ins: &'static Arc<dyn Suit> = Box::leak(ins);

    // let outc = Box::new(outc);
    // let outc: &'static Arc<dyn Suit> = Box::leak(outc);

    tokio::select! {
        r = async {
            loop {
                let (tcpstream, raddr) = listener.accept().await?;


                let laddr = laddr.clone();
                let ti = clone_oti();
                let iiter = ins.get_mappers_vec().into_iter();
                let oiter =  outc.get_mappers_vec().into_iter();

                tokio::spawn(async move {
                    if log_enabled!(Debug) {
                        debug!("new tcp in, laddr:{}, raddr: {:?}", laddr, raddr);
                    }

                    let _ = handle_conn(
                        Box::new(tcpstream),
                        ins.whole_name(),
                        outc.whole_name(),
                        raddr.to_string(),
                        "tcp",
                        Box::new(iiter),
                        Box::new(oiter),
                        outc.addr(),
                        ti,
                    )
                    .await;
                });

            }

        } => {
            r

        }
        _ = shutdown_rx => {
            info!("terminating accept loop, {}",wn);
            Ok(())
        }
    }
}

/// 初级监听为 net::Conn 的 链式转发.
///
/// ins指 in_server, outc 指 out client;
/// ins_name 只用于 log 和 debug.
/// ins 的行为只与 ins_iterator 有关, 与name无关. outc 同理。
///
/// 若 outc_addr 为None 或 outc_iterator.next() 必返回 None, 则说明 该 outc 为 direct直连
///
///  会在 最长 READ_HANDSHAKE_TIMEOUT 秒内 进行握手.
/// 握手后, 不会阻塞, 拷贝会在新的异步任务中完成
///
///
pub async fn handle_conn<'a>(
    in_conn: net::Conn,
    ins_name: &str,
    outc_name: &str,
    in_raddr: String,
    network_str: &'static str,

    ins_iterator: MIterBox,

    outc_iterator: MIterBox,
    outc_addr: Option<net::Addr>,
    ti: Option<Arc<net::TransmissionInfo>>,
) -> io::Result<()> {
    let cid = CID::new_by_opti(ti.clone());

    info!("{cid}, new tcp in,{network_str} {in_raddr}, {ins_name} -> {outc_name}");

    let cidc = cid.clone();
    let listen_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            acc2::accumulate(
                cidc,
                ProxyBehavior::DECODE,
                MapResult::c(in_conn),
                ins_iterator,
            )
            .await
        })
        .await;

    let mut listen_result = match listen_result {
        Ok(lr) => lr,
        Err(e) => {
            warn!("{cid}, handshake in server failed with io::Error, {e}");

            return Err(e.into());
        }
    };

    let target_addr = match listen_result.a.take() {
        Some(ta) => ta,
        None => {
            warn!("{cid}, handshake in server succeed but got no target_addr",);
            let _ = listen_result.c.try_shutdown().await;
            return Err(io::Error::other(
                "handshake in server succeed but got no target_addr",
            ));
        }
    };

    if log_enabled!(log::Level::Info) {
        info!(
            "{cid}, handshake in server succeed, target_addr: {}",
            &target_addr
        )
    }

    let is_direct: bool;

    let real_target_addr = match outc_addr {
        Some(oa) => {
            is_direct = false;
            oa
        }
        None => {
            is_direct = true;
            target_addr.clone()
        }
    };

    //todo: DNS 功能

    let out_stream = match real_target_addr.try_dial().await {
        Ok(t) => t,
        Err(e) => {
            warn!("{cid}, parse target addr failed, {real_target_addr} , {e}",);
            let _ = listen_result.c.try_shutdown().await;
            return Err(e);
        }
    };

    if is_direct {
        cp_stream(cid, listen_result.c, out_stream, listen_result.b.take(), ti);
    } else {
        let cidc = cid.clone();
        let dial_result =
            tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
                acc2::accumulate(
                    cidc,
                    ProxyBehavior::ENCODE,
                    MapResult {
                        a: Some(target_addr),
                        b: listen_result.b.take(),
                        c: out_stream,
                        d: None,
                        e: None,
                        new_id: None,
                    },
                    outc_iterator,
                )
                .await
            })
            .await;

        let dial_result = match dial_result {
            Ok(r) => r,
            Err(e) => {
                warn!("{cid}, dial out client timeout, {e}",);
                return Err(e.into());
            }
        };

        if let Some(e) = dial_result.e {
            warn!("{cid}, dial out client failed, {e}",);
            return Err(e);
        }
        if let Stream::None = dial_result.c {
            warn!("{cid}, dial out client stream got consumed ",);

            return Ok(());
        }

        if let Some(rta) = dial_result.a {
            warn!("{cid}, dial out client succeed, but the target_addr is not consumed, {rta} ",);
        }
        cp_stream(cid, listen_result.c, dial_result.c, None, ti);
    }

    Ok(())
}
