use std::{
    cmp::min,
    collections::HashMap,
    io,
    os::fd::AsRawFd,
    pin::Pin,
    sync::{atomic::AtomicBool, Arc},
    task::{Context, Poll},
};

use bytes::{Buf, BytesMut};
use futures::{channel::oneshot, Future};
use ruci::{
    net::{self, MTU},
    Name,
};
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    Mutex,
};
use tracing::{debug, warn};

use crate::net::{
    so2::{self, SockOpt},
    so_opts::tproxy_udp_recv_from_with_destination,
};

use super::{
    addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr, MAX_DATAGRAM_SIZE},
    Addr, NetAddr, Network,
};

/// (buf_index, left_bound, right_bound), dst, src
type DataIndexDstSrc = ((usize, usize, usize), net::Addr, net::Addr);

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

            let r = tx.try_send(((current_buf_i, left_bound, left_bound + n), dst_a, src_a));
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

        tokio::spawn(async move {
            let conn_map: Arc<Mutex<HashMap<(Addr, Addr), Sender<BytesMut>>>> =
                Arc::new(Mutex::new(HashMap::new()));
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx=>{
                        debug!("tproxy UdpListener got shutdown, will break");
                        break;
                    }

                    r =  udp_msg_rx.recv() =>{
                        let ((i,lb,rb), dst,src) = match r {
                            Some(r) => r,
                            None => {
                                debug!("tproxy UdpListener loop rx got none, will break");
                                break;
                            }
                        };

                        let b = unsafe {
                            if i == 0 {
                                &mut BUF1
                            } else {
                                &mut BUF2
                            }
                        };
                        let buf = &b[lb..rb];

                        let mut map_mg = conn_map.lock().await;
                        let k = (dst.clone(),src.clone());

                        if map_mg.contains_key(&k) {

                            let new_buf = BytesMut::from(buf);

                            let msg_tx = map_mg.get(&k).unwrap();
                            let r = msg_tx.send(new_buf).await;
                            if let Err(e) = r {
                                debug!("tproxy UdpListener tx send got e: {e}");
                                map_mg.remove(&k);
                                continue;
                            }
                        } else {
                            let (msg_tx, msg_rx) = mpsc::channel(100);

                            map_mg.insert(k.clone(), msg_tx);
                            let first_buf = BytesMut::from(buf);

                            let ac = new_addr_conn(
                                msg_rx,
                                src.clone(),
                                dst.clone(),
                                conn_map.clone(),
                            );

                            let r = new_ac_tx.send(AcceptData{ac,dst, src,first_buf}).await;
                            if let Err(e) = r {
                                debug!("tproxy UdpListener loop got e: {e}");
                                break;
                            }
                        }

                    }
                }
            } //loop
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
fn new_addr_conn(
    r: Receiver<BytesMut>,
    src: Addr,
    dst: Addr,

    conn_map: Arc<Mutex<HashMap<(Addr, Addr), Sender<BytesMut>>>>,
) -> AddrConn {
    let r = Reader {
        dst: dst.clone(),
        rx: r,
        last_buf: None,
        state: ReadState::Buf,
    };
    let w = Writer { src, dst, conn_map };
    let mut ac = AddrConn::new(Box::new(r), Box::new(w));
    ac.cached_name = String::from("tproxy_udp");
    ac
}

pub struct Writer {
    src: Addr,
    dst: Addr,
    conn_map: Arc<Mutex<HashMap<(Addr, Addr), Sender<BytesMut>>>>,
}
impl Name for Writer {
    fn name(&self) -> &str {
        "tproxy_udp_w"
    }
}

impl AsyncWriteAddr for Writer {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
        dst: &Addr,
    ) -> Poll<io::Result<usize>> {
        //debug!("tproxy_udp_w write called {} {dst} {}", buf.len(), self.src);

        let us = so2::connect_tproxy_udp(dst, &self.src).unwrap();

        let r = us.send(buf);

        // socket2 is automatically closed when dropped

        // let fd = us.as_raw_fd();
        // unsafe {
        //     libc::close(fd);
        // }

        // 没必要 用 一个 "hashmap with timeout" 缓存. 因为 udp 最一般的用途是 dns,
        // 而 dns 都是一次性的 连接

        // 如果要大量传 udp 单连接 大数据, 用 tun 是更好的选择

        Poll::Ready(r)
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close_addr(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let lock_future = self.conn_map.lock();

        match std::pin::pin!(lock_future).poll(cx) {
            Poll::Ready(mut map) => {
                map.remove(&(self.dst.clone(), self.src.clone()));
                //debug!("tproxy_udp_w got closed, removed from conn map {}", self.src);

                // 移除 tx 后 (drop了), Reader 端的 rx 也会自动失效

                Poll::Ready(Ok(()))
            }
            Poll::Pending => {
                debug!("tproxy_udp_w got closed, pending lock");

                Poll::Pending
            }
        }
    }
}

pub struct Reader {
    rx: Receiver<BytesMut>,
    dst: Addr,
    last_buf: Option<BytesMut>,
    state: ReadState,
}
impl ruci::Name for Reader {
    fn name(&self) -> &str {
        "tproxy_udp_w"
    }
}

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
                    if let Some(mut b) = self.last_buf.take() {
                        let r_len = b.len();

                        let min_l = min(r_len, buf.len());

                        b.copy_to_slice(&mut buf[..min_l]);

                        if b.is_empty() {
                            self.state = ReadState::Rx;
                        } else {
                            self.last_buf = Some(b);
                        }

                        return Poll::Ready(Ok((r_len, self.dst.clone())));
                    } else {
                        self.state = ReadState::Rx;
                    }
                }
                ReadState::Rx => {
                    let r = self.rx.poll_recv(cx);
                    match r {
                        Poll::Ready(rx) => match rx {
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
                        },
                        Poll::Pending => return Poll::Pending,
                    }
                }
            } //match
        } //loop
    }
}
