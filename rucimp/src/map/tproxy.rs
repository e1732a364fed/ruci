/*!
Tproxy related Mapper. Tproxy is shortcut for transparent proxy,

Only support linux
 */
use std::cmp::min;
use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::process::Command;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_trait::async_trait;
use bytes::{Buf, BytesMut};
use itertools::Itertools;
use parking_lot::Mutex;
use ruci::map::{self, *};
use ruci::net::addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr};
use ruci::utils::io_error;
use ruci::{
    net::{self, *},
    Name,
};

use macro_mapper::{mapper_ext_fields, MapperExt};
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::net::so2::{self, SockOpt};
use crate::net::so_opts::tproxy_recv_from_with_destination;
use crate::utils::{run_command, sync_run_command_list_no_stop, sync_run_command_list_stop};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Options {
    pub port: Option<u32>,
    pub auto_route: Option<bool>,
    pub auto_route_tcp: Option<bool>,
}

/// TproxyResolver 从 系统发来的 tproxy 相关的 连接
/// 解析出实际 target_addr
#[mapper_ext_fields]
#[derive(Debug, Clone, Default, MapperExt)]
pub struct TcpResolver {
    //opts: Options,
    port: Option<u32>,
}

impl Name for TcpResolver {
    fn name(&self) -> &'static str {
        "tproxy_resolver"
    }
}

fn run_tcp_route(port: u32, also_udp: bool) -> anyhow::Result<()> {
    let list = r#"ip rule add fwmark 1 table 100
ip route add local 0.0.0.0/0 dev lo table 100
iptables -t mangle -N rucimp
iptables -t mangle -A rucimp -d 127.0.0.1/32 -j RETURN
iptables -t mangle -A rucimp -d 224.0.0.0/4 -j RETURN
iptables -t mangle -A rucimp -d 255.255.255.255/32 -j RETURN
iptables -t mangle -A rucimp -d 192.168.0.0/16 -p tcp -j RETURN"#;

    let list = list.split('\n').collect_vec();

    sync_run_command_list_stop(list)?;

    //rucimp , proxy other devices
    // rucimp_self , proxy self

    if also_udp {
        run_command(
            "iptables",
            "-t mangle -A rucimp -d 192.168.0.0/16 -p udp -j RETURN",
        )?;
    }

    run_command(
        "iptables",
        format!("-t mangle -A rucimp -p tcp -j TPROXY --on-port {port} --tproxy-mark 1").as_str(),
    )?;

    if also_udp {
        run_command(
            "iptables",
            format!("-t mangle -A rucimp -p udp -j TPROXY --on-port {port} --tproxy-mark 1")
                .as_str(),
        )?;
    }

    let list = r#"iptables -t mangle -A PREROUTING -j rucimp
iptables -t mangle -N rucimp_self
iptables -t mangle -A rucimp_self -d 224.0.0.0/4 -j RETURN
iptables -t mangle -A rucimp_self -d 255.255.255.255/32 -j RETURN
iptables -t mangle -A rucimp_self -d 192.168.0.0/16 -p tcp -j RETURN"#;

    let list = list.split('\n').collect_vec();

    sync_run_command_list_stop(list)?;

    if also_udp {
        run_command(
            "iptables",
            "-t mangle -A rucimp_self -d 192.168.0.0/16 -p udp -j RETURN",
        )?;
    }

    run_command(
        "iptables",
        "-t mangle -A rucimp_self -j RETURN -m mark --mark 0xff",
    )?;

    run_command(
        "iptables",
        "-t mangle -A rucimp_self -p tcp -j MARK --set-mark 1",
    )?;

    if also_udp {
        run_command(
            "iptables",
            "-t mangle -A rucimp_self -p udp -j MARK --set-mark 1",
        )?;
    }

    //apply

    run_command("iptables", "-t mangle -A OUTPUT -j rucimp_self")?;

    Ok(())
}

fn down_auto_route(port: u32) -> anyhow::Result<()> {
    let list = format!(
        r#"ip rule del fwmark 1 table 100
ip route del local 0.0.0.0/0 dev lo table 100
iptables -t mangle -D rucimp -d 127.0.0.1/32 -j RETURN
iptables -t mangle -D rucimp -d 224.0.0.0/4 -j RETURN
iptables -t mangle -D rucimp -d 255.255.255.255/32 -j RETURN
iptables -t mangle -D rucimp -d 192.168.0.0/16 -p tcp -j RETURN
iptables -t mangle -D rucimp -d 192.168.0.0/16 -p udp -j RETURN
iptables -t mangle -D rucimp -p udp -j TPROXY --on-port {port} --tproxy-mark 1
iptables -t mangle -D rucimp -p tcp -j TPROXY --on-port {port} --tproxy-mark 1
iptables -t mangle -D PREROUTING -j rucimp
iptables -t mangle -D rucimp_self -d 224.0.0.0/4 -j RETURN
iptables -t mangle -D rucimp_self -d 255.255.255.255/32 -j RETURN
iptables -t mangle -D rucimp_self -d 192.168.0.0/16 -p tcp -j RETURN
iptables -t mangle -D rucimp_self -d 192.168.0.0/16 -p udp -j RETURN
iptables -t mangle -D rucimp_self -j RETURN -m mark --mark 0xff
iptables -t mangle -D rucimp_self -p udp -j MARK --set-mark 1
iptables -t mangle -D rucimp_self -p tcp -j MARK --set-mark 1
iptables -t mangle -D OUTPUT -j rucimp_self
iptables -t mangle -F rucimp
iptables -t mangle -X rucimp
iptables -t mangle -F rucimp_self
iptables -t mangle -X rucimp_self"#
    );
    let list: Vec<_> = list.split('\n').collect();
    sync_run_command_list_no_stop(list)?;

    Ok(())
}

impl TcpResolver {
    pub fn new(opts: Options) -> anyhow::Result<Self> {
        if opts.auto_route.unwrap_or_default() {
            info!("tproxy run auto_route");

            anyhow::Context::context(
                run_tcp_route(opts.port.unwrap_or(12345), true),
                "run auto_route commands failed",
            )?;
        } else if opts.auto_route_tcp.unwrap_or_default() {
            info!("tproxy run auto_route_tcp");

            anyhow::Context::context(
                run_tcp_route(opts.port.unwrap_or(12345), false),
                "run auto_route_tcp commands failed",
            )?;
        }

        let is_auto_route =
            opts.auto_route.unwrap_or_default() || opts.auto_route_tcp.unwrap_or_default();
        Ok(Self {
            port: if is_auto_route { opts.port } else { None },
            ext_fields: Some(MapperExtFields::default()),
        })
    }
}

impl Drop for TcpResolver {
    fn drop(&mut self) {
        if let Some(port) = self.port {
            info!("tproxy run down_auto_route");

            let r = down_auto_route(port);
            if let Err(e) = r {
                warn!("tproxy run down_auto_route got error {e}")
            }
        }
    }
}

fn get_laddr_from_vd(vd: Vec<Option<Box<dyn Data>>>) -> Option<ruci::net::Addr> {
    for vd in vd.iter().flatten() {
        let oa = vd.get_laddr();
        if oa.is_some() {
            return oa;
        }
    }
    None
}

#[async_trait]
impl Mapper for TcpResolver {
    /// TcpResolver only has decode behavior
    ///
    async fn maps(&self, _cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        if let ProxyBehavior::ENCODE = behavior {
            return MapResult::err_str("tproxy TcpResolver doesn't support ENCODE behavior");
        }
        match params.c {
            Stream::Conn(c) => {
                let oa = get_laddr_from_vd(params.d);

                if oa.is_none() {
                    return MapResult::err_str(
                        "tproxy TcpResolver needs data for local_addr, did't get it from the data.",
                    );
                }
                //debug!(cid = %cid, a=?oa, "tproxy TcpResolver got target_addr: ");

                // laddr in tproxy is in fact target_addr
                MapResult::new_c(c).a(oa).b(params.b).build()
            }
            _ => MapResult::err_str(&format!(
                "tproxy TcpResolver needs a tcp stream, got {}",
                params.c
            )),
        }
    }
}

#[mapper_ext_fields]
#[derive(Clone, Debug, Default, MapperExt)]
pub struct UDPListener {
    pub sopt: SockOpt,
}

impl Name for UDPListener {
    fn name(&self) -> &'static str {
        "tproxy_udp_listener"
    }
}

impl UDPListener {
    pub async fn listen(&self, listen_a: &net::Addr) -> MapResult {
        let r = match listen_a.network {
            Network::UDP => so2::listen_udp(&listen_a, &self.sopt).await.map(|s| {
                let (tx, rx) = mpsc::channel(100); //todo: adjust this
                let us = Arc::new(s);
                let usc = us.clone();

                let src_dst_map = Arc::new(Mutex::new(HashMap::new()));
                let mapc = src_dst_map.clone();

                let mcr = MsgConnR { rx };

                let mcw = MsgConnW { us, src_dst_map };

                // 阻塞函数要用 新线程 , 否则 程序退出时会卡住

                std::thread::spawn(|| loop_accept_udp(usc, tx, mapc));

                let mut ac = AddrConn::new(Box::new(mcr), Box::new(mcw));
                ac.cached_name = "tproxy_udp".to_string();
                Stream::AddrConn(ac)
            }),
            _ => {
                return MapResult::from_e(anyhow::anyhow!(
                    "tproxy_udp_listener need dial udp, got {}",
                    listen_a.network
                ))
            }
        };
        let fake_b = BytesMut::zeroed(10);

        match r {
            Ok(c) => MapResult::builder()
                .c(c)
                .a(Some(
                    Addr::from_network_addr_str("udp://1.1.1.1:53").expect("ok"),
                ))
                .b(Some(fake_b))
                .build(),
            Err(e) => MapResult::from_e(
                e.context(format!("tproxy_udp_listener dial {} failed", listen_a)),
            ),
        }
    }
}

#[async_trait]
impl Mapper for UDPListener {
    /// use configured addr.
    async fn maps(&self, _cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::None => {
                if let Some(configured_dial_a) = &self.configured_target_addr() {
                    return self.listen(configured_dial_a).await;
                }
                return MapResult::err_str("tproxy_udp_listener can't dial without an address");
            }

            _ => {
                return MapResult::err_str(
                    "tproxy_udp_listener can't dial when a stream already exists",
                )
            }
        }
    }
}

/// tproxy 的 udp 不是 fullcone 的, 而是对称的
pub struct MsgConnR {
    rx: mpsc::Receiver<AddrData>,
}

pub struct MsgConnW {
    us: Arc<UdpSocket>,
    src_dst_map: Arc<Mutex<HashMap<Addr, Addr>>>,
}

impl Name for MsgConnW {
    fn name(&self) -> &'static str {
        "tproxy_udp_w"
    }
}

impl Name for MsgConnR {
    fn name(&self) -> &'static str {
        "tproxy_udp_r"
    }
}

pub type AddrData = (BytesMut, net::Addr);

impl AsyncWriteAddr for MsgConnW {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let real_target = {
            let mg = self.src_dst_map.lock();
            mg.get(addr).cloned()
        };

        let rt = match real_target {
            Some(rt) => rt,
            None => {
                warn!("tproxy udp get from src_dst_map got none, {}", addr);
                return Poll::Ready(Ok(buf.len()));
            }
        };
        self.us.poll_send_to(cx, buf, rt.get_socket_addr().unwrap())
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        debug!("tproxy MsgConnW close called");
        let mut mg = self.src_dst_map.lock();
        mg.clear();
        Poll::Ready(Ok(()))
    }
}

impl AsyncReadAddr for MsgConnR {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let r = self.rx.try_recv();
        match r {
            Err(e) => match e {
                mpsc::error::TryRecvError::Empty => return Poll::Pending,
                mpsc::error::TryRecvError::Disconnected => {
                    return Poll::Ready(Err(io_error("tproxy udp read got disconnected")))
                }
            },

            Ok(mut ad) => {
                let l = ad.0.len();
                let bl = buf.len();
                let len_to_cp = min(bl, l);
                ad.0.copy_to_slice(&mut buf[..len_to_cp]);

                let a = ad.1;
                return Poll::Ready(Ok((len_to_cp, a)));
            }
        }
    }
}

fn loop_accept_udp(
    us: Arc<UdpSocket>,
    tx: mpsc::Sender<AddrData>,
    src_dst_map: Arc<Mutex<HashMap<Addr, Addr>>>,
) {
    loop {
        let mut buf = BytesMut::zeroed(1500);

        debug!("loop_accept_udp");

        let r = tproxy_recv_from_with_destination(&us, &mut buf);
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                warn!("tproxy loop_accept_udp tproxy_recv_from_with_destination got err {e}");
                return;
            }
        };
        debug!("loop_accept_udp got r {:?}", r);

        let (n, src, dst) = r;

        if n != 0 {
            buf.truncate(n);
            let dst_a = Addr {
                addr: NetAddr::Socket(dst),
                network: Network::UDP,
            };
            let src_a = Addr {
                addr: NetAddr::Socket(src),
                network: Network::UDP,
            };

            {
                let mut mg = src_dst_map.lock();
                mg.insert(src_a, dst_a.clone());
            }

            let r = tx.try_send((buf, dst_a));
            if let Err(e) = r {
                warn!("tproxy loop_accept_udp tx.send got err {e}");

                return;
            }
        } else {
            debug!("tproxy loop_accept_udp read got n=0");

            continue;
        }
    }
}
