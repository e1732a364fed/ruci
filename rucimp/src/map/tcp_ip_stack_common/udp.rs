/*
代码修改自 tproxy 模块 中的对应 udp 代码
*/

use std::fmt::{Display, Formatter};
use std::{
    cmp::min,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{atomic::AtomicBool, Arc},
    task::{ready, Context, Poll},
    time::Instant,
};

use bytes::BytesMut;
use futures::channel::oneshot;
use ruci::net::addr_conn::CP_UDP_TIMEOUT;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::{debug, warn};

use ruci::net::{
    addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr},
    Addr, NetAddr, Network,
};

use dashmap::DashMap;

/// data, dst, src
pub type DataDstSrc = (Vec<u8>, SocketAddr, SocketAddr);

const UDP_CHANNEL_SIZE: usize = 4096;
const UDP_CONN_CHANNEL_SIZE: usize = 100;
const UDP_TIMEOUT_MULTIPLIER: u32 = 2;

#[async_trait::async_trait]
pub trait UdpRead: Send {
    async fn read(&mut self) -> io::Result<DataDstSrc>;
}

#[async_trait::async_trait]
pub trait UdpWrite: Send {
    async fn write(&mut self, data: DataDstSrc) -> io::Result<()>;
}

pub async fn loop_accept_udp(
    r: &mut dyn UdpRead,
    tx: mpsc::Sender<DataDstSrc>,
    shutdown_atomic: Arc<AtomicBool>,
) {
    loop {
        if shutdown_atomic.load(std::sync::atomic::Ordering::Relaxed) {
            debug!("stack udp thread shutdown_atomic = true");
            break;
        }

        let r = r.read().await;

        if shutdown_atomic.load(std::sync::atomic::Ordering::Relaxed) {
            debug!("stack udp thread shutdown_atomic = true");

            break;
        }

        let r = match r {
            Ok(r) => {
                tracing::trace!("stack udp thread got {} {} {}", r.0.len(), r.1, r.2);

                r
            }
            Err(_) => {
                warn!("stack loop_accept_udp read got none");
                return;
            }
        };

        let (data, src, dst) = r;

        if !data.is_empty() {
            let r = tx.try_send((data, src, dst));

            if let Err(e) = r {
                warn!(
                    "stack loop_accept_udp tx.send got err: {e}, {} {}",
                    src, dst
                );

                return;
            }
        } else {
            // shouldn't happen
            warn!("stack loop_accept_udp read got n=0, will continue");

            continue;
        }
    }
}

#[derive(Debug)]
pub struct Listener {
    rx: mpsc::Receiver<AcceptData>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

pub struct AcceptData {
    pub ac: AddrConn,
    pub dst: SocketAddr,
    // pub src: SocketAddr,
    pub first_buf: BytesMut,
}

impl Listener {
    pub async fn new(
        sender: Sender<(Vec<u8>, SocketAddr, SocketAddr)>,
        mut udp_rx: Receiver<(Vec<u8>, SocketAddr, SocketAddr)>,
    ) -> anyhow::Result<Self> {
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        let (new_ac_tx, new_ac_rx) = mpsc::channel(UDP_CHANNEL_SIZE);

        let conn_map: ConnMap = Arc::new(DashMap::new());

        // 添加清理任务
        {
            let cleanup_map = conn_map.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(CP_UDP_TIMEOUT);
                loop {
                    interval.tick().await;
                    let now = Instant::now();
                    let timeout = CP_UDP_TIMEOUT * UDP_TIMEOUT_MULTIPLIER;

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
            let conn_map = conn_map.clone();
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx=>{
                        debug!("stack UdpListener got shutdown, will break");
                        break;
                    }

                    r =  udp_rx.recv() =>{
                        let (data,src, dst) = match r {
                            Some(r) => r,
                            None => {
                                debug!("stack UdpListener loop rx got none, will break");
                                break;
                            }
                        };

                        let k = (dst, src);

                        if !conn_map.contains_key(&k) {
                            let (msg_tx, msg_rx) = mpsc::channel(UDP_CONN_CHANNEL_SIZE);

                            let conn_info = ConnInfo {
                                tx: msg_tx,
                                last_active: Instant::now(),
                            };

                            conn_map.insert(k, conn_info);
                            let first_buf = BytesMut::from(data.as_slice());

                            let ac = new_addr_conn(
                                sender.clone(),
                                msg_rx,
                                src ,
                                dst ,
                                conn_map.clone(),
                            );

                            let r = new_ac_tx.send(AcceptData{ac,dst,  first_buf}).await;
                            if let Err(e) = r {
                                debug!("stack UdpListener loop got e: {e}");
                                break;
                            }

                        } else if let Some(mut entry) = conn_map.get_mut(&k) {
                            entry.last_active = Instant::now();
                            let r = entry.tx.send(data).await;
                            if let Err(e) = r {
                                debug!("stack UdpListener tx send got e: {e}");
                                conn_map.remove(&k);
                                continue;
                            }
                        }
                    }
                }
            } //loop
        });

        Ok(Self {
            shutdown_tx: Some(shutdown_tx),
            rx: new_ac_rx,
        })
    }

    pub async fn accept(&mut self) -> anyhow::Result<AcceptData> {
        let ad = self
            .rx
            .recv()
            .await
            .ok_or(anyhow::anyhow!("stack udplistener accept got rx closed"))?;
        Ok(ad)
    }

    /// the listener is not reuseable after shutdown
    pub fn shutdown(&mut self) {
        debug!("stack udp got shutdown called");

        let tx = self.shutdown_tx.take();
        if let Some(tx) = tx {
            debug!("stack udp will shutdown");

            let _ = tx.send(());
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.shutdown()
    }
}

struct ConnInfo {
    tx: Sender<Vec<u8>>,
    last_active: Instant,
}

type ConnMap = Arc<DashMap<(SocketAddr, SocketAddr), ConnInfo>>;

/// init a AddrConn from a UdpSocket
fn new_addr_conn(
    w: Sender<(Vec<u8>, SocketAddr, SocketAddr)>,
    r: Receiver<Vec<u8>>,

    src: std::net::SocketAddr,
    dst: std::net::SocketAddr,
    conn_map: ConnMap,
) -> AddrConn {
    let r = Reader {
        dst,
        rx: r,
        last_buf: None,
        state: ReadState::Buf,
    };
    let w = Writer {
        w,
        src,
        dst,
        conn_map,
    };
    AddrConn::new(Box::new(r), Box::new(w))
}

pub struct Writer {
    w: Sender<(Vec<u8>, SocketAddr, SocketAddr)>,
    src: std::net::SocketAddr,
    dst: std::net::SocketAddr,
    conn_map: ConnMap,
}
impl Display for Writer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "stack_udp_w")
    }
}

impl AsyncWriteAddr for Writer {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
        dst: &Addr,
    ) -> Poll<io::Result<usize>> {
        let k = (self.dst, self.src);
        if let Some(mut entry) = self.conn_map.get_mut(&k) {
            entry.last_active = Instant::now();
        }

        let r = self
            .w
            .try_send((buf.to_vec(), dst.get_socket_addr().unwrap(), self.src));
        match r {
            Ok(_) => Poll::Ready(Ok(buf.len())),
            Err(e) => Poll::Ready(Err(io::Error::other(e))),
        }
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.conn_map.remove(&(self.dst, self.src));
        Poll::Ready(Ok(()))
    }
}

pub struct Reader {
    rx: Receiver<Vec<u8>>,
    dst: std::net::SocketAddr,
    last_buf: Option<Vec<u8>>,
    state: ReadState,
}
impl Display for Reader {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "stack_udp_r")
    }
}

#[derive(Debug)]
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

                        buf[..min_l].copy_from_slice(&b.as_slice()[..min_l]);

                        if min_l == r_len {
                            self.state = ReadState::Rx;
                        } else {
                            b.truncate(r_len - min_l);
                            self.last_buf = Some(b);
                        }

                        return Poll::Ready(Ok((
                            r_len,
                            Addr {
                                addr: NetAddr::Socket(self.dst),
                                network: Network::UDP,
                            },
                        )));
                    } else {
                        self.state = ReadState::Rx;
                    }
                }
                ReadState::Rx => {
                    let r = self.rx.poll_recv(cx);
                    match ready!(r) {
                        Some(b) => {
                            self.last_buf = Some(b);
                            self.state = ReadState::Buf;
                        }
                        None => {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::ConnectionAborted,
                                "stack_udp read got rx closed",
                            )))
                        }
                    }
                }
            } //match
        } //loop
    }
}
