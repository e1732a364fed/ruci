use std::{fmt::Display, time::Duration};

use super::*;
use log::warn;
use ruci::{
    map,
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

/// 实现 State, 其没有 parent
#[derive(Debug, Default, Clone)]
pub struct RootState {
    pub cid: u32, //固定十进制位数的数
    pub network: &'static str,
    pub ins_name: String,        // 入口名称
    pub outc_name: String,       // 出口名称
    pub cached_in_raddr: String, // 进入程序时的 连接 的远端地址
}

impl RootState {
    /// new with random id
    pub fn new(network: &'static str) -> RootState {
        let mut s = RootState::default();
        s.cid = net::new_rand_cid();
        s.network = network;
        s
    }

    /// new with ordered id
    pub fn new_ordered(network: &'static str, lastid: &std::sync::atomic::AtomicU32) -> RootState {
        let li = lastid.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut s = RootState::default();
        s.cid = li + 1;
        s.network = network;
        s
    }
}

impl Display for RootState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.outc_name() == "" {
            write!(
                f,
                "[ {}, {}://{}, listener: {}, ]  ",
                self.cid(),
                self.network(),
                self.cached_in_raddr(),
                self.ins_name()
            )
        } else {
            write!(
                f,
                "[ {}, {}://{}, route from: {}, to: {} ]  ",
                self.cid(),
                self.network(),
                self.cached_in_raddr(),
                self.ins_name(),
                self.outc_name(),
            )
        }
    }
}

impl State<u32> for RootState {
    fn cid(&self) -> CID {
        CID::Unit(self.cid)
    }

    fn network(&self) -> &'static str {
        self.network
    }

    fn ins_name(&self) -> String {
        self.ins_name.clone()
    }

    fn outc_name(&self) -> String {
        self.outc_name.clone()
    }

    fn cached_in_raddr(&self) -> String {
        self.cached_in_raddr.clone()
    }

    fn set_ins_name(&mut self, str: String) {
        self.ins_name = str
    }

    fn set_outc_name(&mut self, str: String) {
        self.outc_name = str
    }

    fn set_cached_in_raddr(&mut self, str: String) {
        self.cached_in_raddr = str
    }
}

/// 阻塞监听 ins。
///
/// 确保调用 listen_ser 前, ins 和 outc 的
/// generate_upper_mappers 方法被调用过了
pub async fn listen_ser(
    ins: Arc<dyn Suit>,
    outc: Arc<dyn Suit>,
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
    ins: Arc<dyn Suit>,
    outc: Arc<dyn Suit>,
    oti: Option<Arc<net::TransmissionInfo>>,
    shutdown_rx: oneshot::Receiver<()>,
) -> io::Result<()> {
    let laddr = ins.addr_str().to_string();
    let wn = ins.whole_name().to_string();
    info!("start listen tcp {}, {}", laddr, wn);

    let listener = TcpListener::bind(laddr.clone()).await?;

    let clone_oti = move || oti.clone();

    let ins = Box::new(ins);
    let ins: &'static Arc<dyn Suit> = Box::leak(ins);

    let outc = Box::new(outc);
    let outc: &'static Arc<dyn Suit> = Box::leak(outc);

    tokio::select! {
        r = async {
            loop {
                let r = listener.accept().await;
                if r.is_err(){

                    break;
                }
                let (tcpstream, raddr) = r.unwrap();

                let laddr = laddr.clone();
                let ti = clone_oti();
                let iiter = ins.get_mappers_vec().iter();
                let oiter =  outc.get_mappers_vec().iter();

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

            Ok::<_, io::Error>(())
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
    let mut state = match ti.as_ref() {
        Some(ti) => RootState::new_ordered(network_str, &ti.last_connection_id),
        None => RootState::new(network_str),
    };
    state.set_ins_name(ins_name.to_string());
    state.set_cached_in_raddr(in_raddr);

    let cid = state.cid();

    let cidc = cid.clone();
    let listen_result =
        tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
            map::accumulate(
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
        cp_stream(cid, listen_result.c, out_stream, listen_result.b.take(), ti);
    } else {
        let cidc = cid.clone();
        let dial_result =
            tokio::time::timeout(Duration::from_secs(READ_HANDSHAKE_TIMEOUT), async move {
                map::accumulate(
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
            warn!("{state}, dial out client stream got consumed ",);

            return Ok(());
        }

        //let (out_stream, remain_target_addr, _extra_out_data_vec) = dial_result;
        if let Some(rta) = dial_result.a {
            warn!("{state}, dial out client succeed, but the target_addr is not consumed, {rta} ",);
        }
        cp_stream(cid, listen_result.c, dial_result.c, None, ti);
    }

    Ok(())
}
