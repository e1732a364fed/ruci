use std::{
    cmp::min,
    collections::HashMap,
    io,
    os::fd::AsRawFd,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::{Buf, BytesMut};
use futures::{channel::oneshot, Future};
use ruci::{net, Name};
use socket2::Socket;
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    Mutex,
};
use tracing::{debug, warn};

use crate::net::{
    so2::{self, SockOpt},
    so_opts::tproxy_recv_from_with_destination,
};

use super::{
    addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr, MAX_DATAGRAM_SIZE},
    Addr, NetAddr, Network,
};

// (buf_index, left_bound, right_bound)
type DataDstSrc = ((usize, usize, usize), net::Addr, net::Addr);

static mut VEC: [u8; MAX_DATAGRAM_SIZE] = [0u8; MAX_DATAGRAM_SIZE];
static mut VEC2: [u8; MAX_DATAGRAM_SIZE] = [0u8; MAX_DATAGRAM_SIZE];

/// blocking
pub fn loop_accept_udp<T>(us: &T, tx: mpsc::Sender<DataDstSrc>)
where
    T: AsRawFd,
{
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

        let r = tproxy_recv_from_with_destination(us, buf);
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

/// 监听一个 udp 端口, 对 每一个 新 源 udp 端口发来的连接
/// 新建一个 Stream::AddrConn. 用于 udp 端口转发
#[derive(Debug)]
pub struct Listener {
    laddr: Addr,
    rx: mpsc::Receiver<(AddrConn, Addr, Addr, BytesMut)>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl Listener {
    pub async fn new(listen_a: Addr, sopt: SockOpt) -> anyhow::Result<Self> {
        let udp = so2::block_listen_udp_socket(&listen_a, &sopt)?;

        // let u: tokio::net::UdpSocket =
        //     tokio::net::UdpSocket::from_std(std::net::UdpSocket::from(u))?;

        let (udp_msg_tx, mut udp_msg_rx) = mpsc::channel(1000); //todo: adjust this

        let _jh = std::thread::spawn(move || loop_accept_udp(&udp, udp_msg_tx));

        // tokio::spawn(async move {
        //     let _ = shutdown_rx.await;
        //     info!("tproxy udp got shutdown signal");
        //     // thr.terminate();
        //     // info!("tproxy udp terminated");
        // });

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        let (new_ac_tx, new_ac_rx) = mpsc::channel(1000); //todo: adjust this

        tokio::spawn(async move {
            let conn_map: Arc<Mutex<HashMap<(Addr, Addr), Sender<BytesMut>>>> =
                Arc::new(Mutex::new(HashMap::new()));
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx=>{
                        debug!("tproy UdpListener got shutdown, will break");
                        break;
                    }

                    r =  udp_msg_rx.recv() =>{
                        let ((i,lb,rb), dst,src) = match r {
                            Some(r) => r,
                            None => {
                                debug!("tproy UdpListener loop rx got none, will break");
                                break;
                            }
                        };

                        let b = unsafe {
                            if i == 0 {
                                &mut VEC
                            } else {
                                &mut VEC2
                            }
                        };
                        let buf2 = &b[lb..rb];

                        let mut mg = conn_map.lock().await;
                        let k = (dst.clone(),src.clone());

                        if mg.contains_key(&k) {

                            let new_buf = BytesMut::from(buf2);

                            let tx = mg.get(&k).unwrap();
                            let r = tx.send(new_buf).await;
                            if let Err(e) = r {
                                debug!("tproy UdpListener tx send got e: {e}");
                                continue;
                            }
                        } else {
                            let (tx2, rx2) = mpsc::channel(100);

                            mg.insert(k.clone(), tx2);
                            let first_buf = BytesMut::from(buf2);

                            let ac = new_addr_conn(
                                rx2,
                                src.clone(),
                                dst.clone(),
                                conn_map.clone(),
                            );

                            let r = new_ac_tx.send((ac,dst, src,first_buf)).await;
                            if let Err(e) = r {
                                debug!("tproy UdpListener loop got e: {e}");
                                break;
                            }
                        }

                    }
                }
            } //loop
        });

        Ok(Self {
            shutdown_tx: Some(shutdown_tx),
            laddr: listen_a,
            rx: new_ac_rx,
        })
    }

    /// conn, dst, src, first_data
    pub async fn accept(&mut self) -> anyhow::Result<(AddrConn, Addr, Addr, BytesMut)> {
        let (ac, dst, src, buf) = self
            .rx
            .recv()
            .await
            .ok_or(anyhow::anyhow!("tproxy udplistener accept got rx closed"))?;
        Ok((ac, dst, src, buf))
    }

    pub fn shutdown(&mut self) {
        let tx = self.shutdown_tx.take();
        if let Some(tx) = tx {
            let _ = tx.send(());
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
pub fn new_addr_conn(
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
    let w = Writer {
        src,
        dst,
        conn_map,
        back_established_map: HashMap::new(),
    };
    let mut ac = AddrConn::new(Box::new(r), Box::new(w));
    ac.cached_name = String::from("tproxy_udp");
    ac
}

pub struct Writer {
    src: Addr,
    dst: Addr,
    conn_map: Arc<Mutex<HashMap<(Addr, Addr), Sender<BytesMut>>>>,
    back_established_map: HashMap<(Addr, Addr), Socket>,
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

        // let x = self.back_established_map.get(&k);

        let x: Option<()> = None;
        match x {
            Some(_) => {
                //let r = us.send(buf);

                // prevent too many open files
                // if self.back_established_map.len() > LIMIT {
                //     self.back_established_map.clear()
                // }
                //Poll::Ready(r)
                panic!("shit")
            }
            None => {
                let us = so2::connect_tproxy_udp(dst, &self.src).unwrap();

                let r = us.send(buf);

                let fd = us.as_raw_fd();
                unsafe {
                    libc::close(fd);
                }

                //debug!("clear back_established_map ");

                // if self.back_established_map.len() > LIMIT {
                //     self.back_established_map.iter().for_each(|(_, v)| {
                //         let _ = v.shutdown(std::net::Shutdown::Both);

                //         // let s: tokio::net::UdpSocket =
                //         //     tokio::net::UdpSocket::from_std(std::net::UdpSocket::from(v)).unwrap();

                //         //unimplemented!()
                //     });
                //     self.back_established_map.clear()
                // }
                // self.back_established_map.insert(k, us);

                Poll::Ready(r)
            }
        }
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close_addr(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.back_established_map.clear();
        let x = &self.conn_map;
        let f = x.lock();
        let x = Future::poll(std::pin::pin!(f), cx);
        match x {
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
