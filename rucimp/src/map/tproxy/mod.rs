/*!
Tproxy related Map. Tproxy is shortcut for transparent proxy,

Only support linux
 */
pub mod route;

pub use route::*;
use socket2::Socket;

use std::cmp::min;
use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_trait::async_trait;
use bytes::{Buf, BytesMut};
use itertools::Itertools;
use parking_lot::Mutex;
use ruci::map::{self, *};
use ruci::net::addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr};
use ruci::{
    net::{self, *},
    Name,
};

use macro_map::{map_ext_fields, MapExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

use crate::net::so2::{self, SockOpt};
use crate::net::so_opts::tproxy_recv_from_with_destination;
use crate::utils::{run_command, sync_run_command_list_no_stop, sync_run_command_list_stop};

/// TproxyResolver 从 系统发来的 tproxy 相关的 连接
/// 解析出实际 target_addr
#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct TcpResolver {
    opts: Options,
}

impl Name for TcpResolver {
    fn name(&self) -> &'static str {
        "tproxy_resolver"
    }
}

impl TcpResolver {
    pub fn new(opts: Options) -> anyhow::Result<Self> {
        if opts.auto_route_enabled() {
            info!("tproxy run auto_route");

            anyhow::Context::context(run_auto_route(&opts), "run auto_route commands failed")?;

            if opts.route_ipv6.unwrap_or_default() {
                info!("tproxy run_tcp_route6");
                let r = run_auto_route6(&opts);
                if let Err(e) = r {
                    warn!("tproxy run run_tcp_route6 got error {e}")
                }
            }
        }

        Ok(Self {
            opts,
            ext_fields: Some(MapExtFields::default()),
        })
    }
}

impl Drop for TcpResolver {
    fn drop(&mut self) {
        if self.opts.auto_route_enabled() {
            info!("tproxy run down_auto_route");

            let r = down_auto_route(&self.opts);
            if let Err(e) = r {
                warn!("tproxy run down_auto_route got error {e}")
            }

            if self.opts.route_ipv6.unwrap_or_default() {
                let r = down_auto_route6(&self.opts);
                if let Err(e) = r {
                    warn!("tproxy run down_auto_route6 got error {e}")
                }
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
impl Map for TcpResolver {
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

#[map_ext_fields]
#[derive(Clone, Debug, Default, MapExt)]
pub struct UDPListener {
    pub listen_addr: net::Addr,
    pub sopt: SockOpt,
}

impl Name for UDPListener {
    fn name(&self) -> &'static str {
        "tproxy_udp_listener"
    }
}

impl UDPListener {
    pub async fn listen(
        &self,
        listen_a: &net::Addr,
        shutdown_rx: oneshot::Receiver<()>,
    ) -> MapResult {
        let (shutdown_addrconn_tx, shutdown_addrconn_rx) = oneshot::channel();

        let r = match listen_a.network {
            Network::UDP => so2::block_listen_udp_socket(&listen_a, &self.sopt).map(|socket| {
                let (tx, rx) = mpsc::channel(1000); //todo: adjust this

                let dst_src_map = Arc::new(Mutex::new(HashMap::new()));
                let mapc = dst_src_map.clone();

                let r = UdpR::new(rx, mapc);

                let w = UdpW {
                    dst_src_map,
                    ..Default::default()
                };

                // 阻塞函数要用 新线程 而不是 tokio::spawn, 否则 程序退出时会卡住

                // use terminate_thread::Thread;
                // let thr = Thread::spawn(|| loop_accept_udp(socket, tx));

                let _jh = std::thread::spawn(|| loop_accept_udp(socket, tx));

                tokio::spawn(async move {
                    let _ = shutdown_rx.await;
                    info!("tproxy udp got shutdown signal");
                    let _ = shutdown_addrconn_tx.send(());
                    // thr.terminate();
                    // info!("tproxy udp terminated");
                });

                let mut ac = AddrConn::new(Box::new(r), Box::new(w));
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

        // provide a fake request, as tproxy udp doesn't have first request

        let fake_b = BytesMut::zeroed(10);

        match r {
            Ok(c) => MapResult::builder()
                .c(c)
                .a(Some(
                    Addr::from_network_addr_url("udp://1.1.1.1:53").expect("ok"),
                ))
                .b(Some(fake_b))
                .shutdown_rx(shutdown_addrconn_rx)
                .build(),
            Err(e) => MapResult::from_e(
                e.context(format!("tproxy_udp_listener dial {} failed", listen_a)),
            ),
        }
    }
}

#[async_trait]
impl Map for UDPListener {
    /// use configured addr.
    async fn maps(&self, _cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::None => {
                return self.listen(&self.listen_addr, params.shutdown_rx.expect("tproxy_udp_listener requires a shutdown_rx to support graceful shutdown")).await;
            }

            _ => {
                return MapResult::err_str(
                    "tproxy_udp_listener can't dial when a stream already exists",
                )
            }
        }
    }
}

pub struct UdpR {
    rx: mpsc::Receiver<DataDstSrc>,
    dst_src_map: Arc<Mutex<HashMap<Addr, Addr>>>,
}
impl UdpR {
    fn new(rx: mpsc::Receiver<DataDstSrc>, dst_src_map: Arc<Mutex<HashMap<Addr, Addr>>>) -> Self {
        Self { rx, dst_src_map }
    }
}

/// tproxy 的 udp 不是 fullcone 的, 而是对称的
#[derive(Default)]
pub struct UdpW {
    dst_src_map: Arc<Mutex<HashMap<Addr, Addr>>>,

    established_map: HashMap<(Addr, Addr), Socket>,
}

impl Name for UdpW {
    fn name(&self) -> &'static str {
        "tproxy_udp_w"
    }
}

impl Name for UdpR {
    fn name(&self) -> &'static str {
        "tproxy_udp_r"
    }
}

// (buf_index, left_bound, right_bound)
type DataDstSrc = ((usize, usize, usize), net::Addr, net::Addr);

/// 将 外来 的udp 数据 写回 本机
impl AsyncWriteAddr for UdpW {
    fn poll_write_addr(
        mut self: Pin<&mut Self>,
        _cx: &mut Context,
        buf: &[u8],
        raddr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let laddr = {
            let mg = self.dst_src_map.lock();
            mg.get(raddr).cloned()
        };

        let laddr = match laddr {
            Some(laddr) => laddr,
            None => {
                warn!("tproxy UdpW get from dst_src_map got none, {}", raddr);
                return Poll::Ready(Ok(buf.len()));
            }
        };

        // 实测这里不能 将 socket2::Socket 转成 tokio 的 UdpSocket使用,  否 则 poll send 会一直为 pending

        let k = (laddr, raddr.clone());
        let x = self.established_map.get(&k);
        //debug!("self.established_map {}", self.established_map.len());

        const LIMIT: usize = 200;
        match x {
            Some(us) => {
                let r = us.send(buf);

                // prevent too many open files
                if self.established_map.len() > LIMIT {
                    //debug!("self.established_map.clear()");
                    self.established_map.clear()
                }
                Poll::Ready(r)
            }
            None => {
                let us = so2::connect_tproxy_udp(raddr, &k.0).unwrap();

                let r = us.send(buf);

                if self.established_map.len() > LIMIT {
                    //debug!("self.established_map.clear() 2");

                    self.established_map.clear()
                }
                self.established_map.insert(k, us);

                Poll::Ready(r)
            }
        }
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close_addr(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        debug!("tproxy UdpW close called");
        self.established_map.clear();

        let mut mg = self.dst_src_map.lock();
        mg.clear();
        Poll::Ready(Ok(()))
    }
}

/// 读取 向外的udp 请求
impl AsyncReadAddr for UdpR {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let r = self.rx.poll_recv(cx);

        match r {
            Poll::Ready(r) => match r {
                Some(ad) => {
                    let (current_using_i, left_bound, right_bound) = ad.0;

                    let data_l = right_bound - left_bound;

                    let bl = buf.len();
                    let len_to_cp = min(bl, data_l);
                    if data_l != len_to_cp {
                        debug!(
                            "tproxy UdpR try recv will short write {} {}",
                            data_l, len_to_cp
                        );
                    }

                    let b = unsafe {
                        if current_using_i == 0 {
                            &mut VEC
                        } else {
                            &mut VEC2
                        }
                    };
                    let mut buf2 = &b[left_bound..right_bound];

                    buf2.copy_to_slice(&mut buf[..len_to_cp]);

                    let dst = ad.1;

                    {
                        let mut mg = self.dst_src_map.lock();
                        mg.insert(dst.clone(), ad.2)
                    };

                    return Poll::Ready(Ok((len_to_cp, dst)));
                }
                None => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "tproxy UdpR rx closed",
                    )));
                }
            },
            Poll::Pending => return Poll::Pending,
        }
    }
}

use ruci::net::addr_conn::MAX_DATAGRAM_SIZE;

static mut VEC: [u8; MAX_DATAGRAM_SIZE] = [0u8; MAX_DATAGRAM_SIZE];
static mut VEC2: [u8; MAX_DATAGRAM_SIZE] = [0u8; MAX_DATAGRAM_SIZE];

/// blocking
fn loop_accept_udp(us: Socket, tx: mpsc::Sender<DataDstSrc>) {
    let mut current_using_i = 0;
    let mut last_i = 0;

    loop {
        let b = unsafe {
            if current_using_i == 0 {
                // is actually &mut VEC, followed the compiler prompt for 2024 edition
                // and changed to using addr_of_mut!
                // https://github.com/rust-lang/rust/issues/114447

                &mut *std::ptr::addr_of_mut!(VEC)
            } else {
                &mut *std::ptr::addr_of_mut!(VEC2)
            }
        };
        let buf = &mut b[last_i..last_i + 1500];

        let r = tproxy_recv_from_with_destination(&us, buf);
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                warn!("tproxy loop_accept_udp tproxy_recv_from_with_destination got err {e}");
                return;
            }
        };
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!("tproxy udp thread got {:?}", r);
        }
        // 如 本机请求 dns, 则 src 为 本机ip 随机高端口， dst 为 路由器ip 53 端口
        let (n, src, dst) = r;

        if n != 0 {
            let dst_a = Addr {
                addr: NetAddr::Socket(dst),
                network: Network::UDP,
            };
            let src_a = Addr {
                addr: NetAddr::Socket(src),
                network: Network::UDP,
            };

            let r = tx.try_send(((current_using_i, last_i, last_i + n), dst_a, src_a));
            last_i += n;
            if last_i + 1500 > MAX_DATAGRAM_SIZE {
                last_i = 0;
                current_using_i += 1;
                if current_using_i >= 2 {
                    current_using_i = 0
                }
            }

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
