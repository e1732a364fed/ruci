use std::{
    cmp::min,
    io,
    os::fd::AsRawFd,
    pin::Pin,
    sync::{atomic::AtomicBool, Arc},
    task::{Context, Poll},
    time::Instant,
};

use bytes::{Buf, BytesMut};
use dashmap::DashMap;
use futures::channel::oneshot;
use ruci::net::{self, addr_conn::CP_UDP_TIMEOUT, MTU};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::{debug, warn};

use crate::net::{
    so2::{self, SockOpt},
    so_opts::tproxy_udp_recv_from_with_destination,
};

use super::{
    addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr, MAX_DATAGRAM_SIZE},
    Addr, NetAddr, Network,
};

pub struct DataIndex {
    pub buf_index: usize,
    pub left_bound: usize,
    pub right_bound: usize,
}

///dst, src
type DataIndexDstSrc = (DataIndex, net::Addr, net::Addr);

static mut BUF1: [u8; MAX_DATAGRAM_SIZE] = [0u8; MAX_DATAGRAM_SIZE];
static mut BUF2: [u8; MAX_DATAGRAM_SIZE] = [0u8; MAX_DATAGRAM_SIZE];

/// blocking
pub fn loop_accept_udp<T>(
    us: &T,
    tx: mpsc::Sender<DataIndexDstSrc>,
    shutdown_atomic: Arc<AtomicBool>,
) where
    T: AsRawFd,
{
    let mut current_buf_i = 0;
    let mut left_bound = 0;

    loop {
        if shutdown_atomic.load(std::sync::atomic::Ordering::Relaxed) {
            debug!("tproxy udp thread got shutdown_atomic = true");
            break;
        }
        let b = unsafe {
            if current_buf_i == 0 {
                // is actually &mut BUF1, followed the compiler prompt for 2024 edition
                // and changed to using addr_of_mut!
                // https://github.com/rust-lang/rust/issues/114447

                &mut *std::ptr::addr_of_mut!(BUF1)
            } else {
                &mut *std::ptr::addr_of_mut!(BUF2)
            }
        };
        let buf = &mut b[left_bound..left_bound + MTU];

        let r = tproxy_udp_recv_from_with_destination(us, buf);

        if shutdown_atomic.load(std::sync::atomic::Ordering::Relaxed) {
            debug!("tproxy udp thread got shutdown_atomic = true");

            break;
        }

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

            let r = tx.try_send((
                DataIndex {
                    buf_index: current_buf_i,
                    left_bound,
                    right_bound: left_bound + n,
                },
                dst_a,
                src_a,
            ));
            left_bound += n;
            if left_bound + MTU > MAX_DATAGRAM_SIZE {
                left_bound = 0;
                current_buf_i += 1;
                if current_buf_i >= 2 {
                    current_buf_i = 0
                }
            }

            if let Err(e) = r {
                warn!("tproxy loop_accept_udp tx.send got err {e}");

                return;
            }
        } else {
            // shouldn't happen
            warn!("tproxy loop_accept_udp read got n=0, will continue");

            continue;
        }
    }
}

/// 监听一个 udp 端口, 对 每一个 新 源 udp 端口发来的连接
/// 新建一个 Stream::AddrConn. 用于 udp 端口转发
#[derive(Debug)]
pub struct Listener {
    laddr: Addr,
    rx: mpsc::Receiver<AcceptData>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    shutdown_thread_atomic: Arc<AtomicBool>,
    fd: i32,
}

pub struct AcceptData {
    pub ac: AddrConn,
    pub dst: Addr,
    pub src: Addr,
    pub first_buf: BytesMut,
}

/// 新增连接信息结构体
struct ConnInfo {
    tx: Sender<DataIndex>,
    last_active: Instant,
}

/// 修改 ConnMap 类型定义
type ConnMap = Arc<DashMap<(Addr, Addr), ConnInfo>>;

impl Listener {
    pub async fn new(listen_a: Addr, sopt: SockOpt) -> anyhow::Result<Self> {
        let udp = so2::block_listen_udp_socket(&listen_a, &sopt)?;
        let fd = udp.as_raw_fd();

        let (udp_msg_tx, mut udp_msg_rx) = mpsc::channel(4096);
        let shutdown_thread_atomic = Arc::new(AtomicBool::new(false));

        {
            let stac = shutdown_thread_atomic.clone();
            std::thread::spawn(move || loop_accept_udp(&udp, udp_msg_tx, stac));
        }

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        let (new_ac_tx, new_ac_rx) = mpsc::channel(4096);

        let conn_map: ConnMap = Arc::new(DashMap::new());

        // 添加清理任务
        {
            let cleanup_map = conn_map.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(CP_UDP_TIMEOUT);
                loop {
                    interval.tick().await;
                    let now = Instant::now();
                    let timeout = CP_UDP_TIMEOUT * 2;

                    // 使用 retain 方法清理过期连接
                    cleanup_map.retain(|_, conn_info| {
                        let is_active = now.duration_since(conn_info.last_active) < timeout;
                        if !is_active {
                            debug!("Removing inactive UDP connection");
                        }
                        is_active
                    });
                }
            });
        }

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        debug!("tproxy UdpListener got shutdown, will break");
                        break;
                    }

                    r = udp_msg_rx.recv() => {
                        let (data_index, dst,src) = match r {
                            Some(r) => r,
                            None => {
                                debug!("tproxy UdpListener loop rx got none, will break");
                                break;
                            }
                        };

                        let k = (dst.clone(),src.clone());

                        let now = Instant::now();

                        if let Some(mut entry) = conn_map.get_mut(&k) {
                            // 更新已存在连接的最后活动时间
                            entry.last_active = now;

                            let r = entry.tx.send(data_index).await;
                            if let Err(e) = r {
                                debug!("tproxy UdpListener tx send got e: {e}");
                                conn_map.remove(&k);
                                continue;
                            }
                        } else {
                            let (msg_tx, msg_rx) = mpsc::channel(100);

                            // 创建新的连接信息
                            let conn_info = ConnInfo {
                                tx: msg_tx,
                                last_active: now,
                            };

                            conn_map.insert(k.clone(), conn_info);

                            let i = data_index.buf_index;
                            let lb = data_index.left_bound;
                            let rb = data_index.right_bound;

                            let b = unsafe {
                                if i == 0 { &mut *std::ptr::addr_of_mut!(BUF1) }
                                else { &mut *std::ptr::addr_of_mut!(BUF2) }
                            };
                            let buf = &b[lb..rb];

                            let first_buf = BytesMut::from(buf);

                            let ac = new_addr_conn(
                                msg_rx,
                                src.clone(),
                                dst.clone(),
                                conn_map.clone(),
                            );

                            let r = new_ac_tx.send(AcceptData{ac,dst,src,first_buf}).await;
                            if let Err(e) = r {
                                debug!("tproxy UdpListener loop got e: {e}");
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            fd,
            shutdown_tx: Some(shutdown_tx),
            laddr: listen_a,
            rx: new_ac_rx,
            shutdown_thread_atomic,
        })
    }

    pub async fn accept(&mut self) -> anyhow::Result<AcceptData> {
        let ad = self
            .rx
            .recv()
            .await
            .ok_or(anyhow::anyhow!("tproxy udplistener accept got rx closed"))?;
        Ok(ad)
    }

    /// the listener is not reuseable after shutdown
    pub fn shutdown(&mut self) {
        debug!("tproxy udp got shutdown called");

        let tx = self.shutdown_tx.take();
        if let Some(tx) = tx {
            debug!("tproxy udp will shutdown");

            self.shutdown_thread_atomic
                .store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = tx.send(());

            unsafe {
                //光 调用 close 是不会令 recvmsg 端返回的, shutdown is necessary.
                // 且 shutdown 后 再调用 close 有10% 左右的几率在 thread read 端 得到报错 Bad file Descriptor, 所以没必要
                libc::shutdown(self.fd, libc::SHUT_RDWR);
            }
        }
    }

    pub fn laddr(&self) -> &Addr {
        &self.laddr
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.shutdown()
    }
}

/// init a AddrConn from a UdpSocket
///
/// 如果 peer_addr 给出, 说明 u 是 connected, 将用 recv 而不是 recv_from,
/// 以及用 send 而不是 send_to
///
fn new_addr_conn(r: Receiver<DataIndex>, src: Addr, dst: Addr, conn_map: ConnMap) -> AddrConn {
    let r = Reader {
        dst: dst.clone(),
        rx: r,
        last_buf: None,
        state: ReadState::Buf,
    };
    let w = Writer { src, dst, conn_map };
    let ac = AddrConn::new(Box::new(r), Box::new(w));
    // ac.cached_name = String::from("tproxy_udp");
    ac
}

pub struct Writer {
    src: Addr,
    dst: Addr,
    conn_map: ConnMap,
}
// impl Name for Writer {
//     fn name(&self) -> &str {
//         "tproxy_udp_w"
//     }
// }

impl AsyncWriteAddr for Writer {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
        dst: &Addr,
    ) -> Poll<io::Result<usize>> {
        // 更新连接的最后活动时间
        if let Some(mut entry) = self.conn_map.get_mut(&(self.dst.clone(), self.src.clone())) {
            entry.last_active = Instant::now();
        }

        // debug!("will write {}", buf.len());
        let us = so2::connect_tproxy_udp(dst, &self.src).unwrap();
        let r = us.send(buf);
        // debug!("  write got {r:?}",);

        Poll::Ready(r)
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.conn_map.remove(&(self.dst.clone(), self.src.clone()));
        Poll::Ready(Ok(()))
    }
}

pub struct Reader {
    rx: Receiver<DataIndex>,
    dst: Addr,
    last_buf: Option<DataIndex>,
    state: ReadState,
}
// impl ruci::Name for Reader {
//     fn name(&self) -> &str {
//         "tproxy_udp_w"
//     }
// }

enum ReadState {
    Buf,
    Rx,
}

impl AsyncReadAddr for Reader {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        loop {
            match self.state {
                ReadState::Buf => {
                    if let Some(di) = self.last_buf.take() {
                        let i = di.buf_index;
                        let lb = di.left_bound;
                        let rb = di.right_bound;

                        let b = unsafe {
                            if i == 0 {
                                &mut *std::ptr::addr_of_mut!(BUF1)
                            } else {
                                &mut *std::ptr::addr_of_mut!(BUF2)
                            }
                        };

                        let mut b = &b[lb..rb];

                        let r_len = b.len();

                        let min_l = min(r_len, buf.len());

                        b.copy_to_slice(&mut buf[..min_l]);

                        if b.is_empty() {
                            self.state = ReadState::Rx;
                        } else {
                            self.last_buf = Some(di);
                        }

                        return Poll::Ready(Ok((r_len, self.dst.clone())));
                    } else {
                        self.state = ReadState::Rx;
                    }
                }
                ReadState::Rx => {
                    // debug!("will read rx");
                    let r = self.rx.poll_recv(cx);
                    // debug!("  read rx got {r:?}");

                    match std::task::ready!(r) {
                        Some(b) => {
                            //debug!("tproxy_udp r read got {}", b.len());
                            self.last_buf = Some(b);
                            self.state = ReadState::Buf;
                        }
                        None => {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::ConnectionAborted,
                                "tproxy_udp read got rx closed",
                            )))
                        }
                    }
                }
            } //match
        } //loop
    }
}
