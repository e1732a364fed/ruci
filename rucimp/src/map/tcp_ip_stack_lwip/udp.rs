/*
代码修改自 tproxy 包中的对应 udp 代码
*/

use std::{
    cmp::min,
    collections::HashMap,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{atomic::AtomicBool, Arc},
    task::{ready, Context, Poll},
    time::Instant,
};

use bytes::BytesMut;
use futures::{channel::oneshot, Future};
use netstack_lwip::udp::RecvHalf;
use ruci::Name;
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    Mutex,
};
use tracing::{debug, warn};

use ruci::net::{
    addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr},
    Addr, NetAddr, Network,
};

/// (buf_index, left_bound, right_bound), dst, src
type DataIndexDstSrc = (Vec<u8>, SocketAddr, SocketAddr);

pub async fn loop_accept_udp(
    mut r: RecvHalf,
    tx: mpsc::Sender<DataIndexDstSrc>,
    shutdown_atomic: Arc<AtomicBool>,
) {
    loop {
        debug!("lwip loop_accept_udp");

        if shutdown_atomic.load(std::sync::atomic::Ordering::Relaxed) {
            debug!("lwip udp thread got shutdown_atomic = true");
            break;
        }

        let r = r.recv_from().await;

        if shutdown_atomic.load(std::sync::atomic::Ordering::Relaxed) {
            debug!("lwip udp thread got shutdown_atomic = true");

            break;
        }

        tracing::debug!("lwip udp thread got {}", r.is_ok());

        let r = match r {
            Ok(r) => r,
            Err(e) => {
                warn!("lwip loop_accept_udp tproxy_recv_from_with_destination got err {e}");
                return;
            }
        };

        let (data, src, dst) = r;

        if !data.is_empty() {
            let r = tx.try_send((data, src, dst));

            if let Err(e) = r {
                warn!("lwip loop_accept_udp tx.send got err {e}");

                return;
            }
        } else {
            // shouldn't happen
            warn!("lwip loop_accept_udp read got n=0, will continue");

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
        let (new_ac_tx, new_ac_rx) = mpsc::channel(4096);

        tokio::spawn(async move {
            let conn_map: ConnMap = Arc::new(Mutex::new(HashMap::new()));
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx=>{
                        debug!("lwip UdpListener got shutdown, will break");
                        break;
                    }

                    r =  udp_rx.recv() =>{
                        let (data,src, dst) = match r {
                            Some(r) => r,
                            None => {
                                debug!("lwip UdpListener loop rx got none, will break");
                                break;
                            }
                        };

                        let mut map_mg = conn_map.lock().await;
                        let k = (dst ,src );

                        if let std::collections::hash_map::Entry::Vacant(e) = map_mg.entry(k) {
                            let (msg_tx, msg_rx) = mpsc::channel(100);

                           // 创建新的 ConnInfo
                            let conn_info = ConnInfo {
                                tx: msg_tx,
                                last_active: Instant::now(),
                            };

                            e.insert(conn_info);
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
                                debug!("lwip UdpListener loop got e: {e}");
                                break;
                            }

                        } else {
                            // 更新已存在连接的最后活动时间
                            if let Some(conn_info) = map_mg.get_mut(&k) {
                                conn_info.last_active = Instant::now();
                                let r = conn_info.tx.send(data).await;
                                if let Err(e) = r {
                                    debug!("lwip UdpListener tx send got e: {e}");
                                    map_mg.remove(&k);
                                    continue;
                                }
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
            .ok_or(anyhow::anyhow!("lwip udplistener accept got rx closed"))?;
        Ok(ad)
    }

    /// the listener is not reuseable after shutdown
    pub fn shutdown(&mut self) {
        debug!("lwip udp got shutdown called");

        let tx = self.shutdown_tx.take();
        if let Some(tx) = tx {
            debug!("lwip udp will shutdown");

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

// 修改 ConnMap 类型定义
type ConnMap = Arc<Mutex<HashMap<(SocketAddr, SocketAddr), ConnInfo>>>;

/// init a AddrConn from a UdpSocket
///
/// 如果 peer_addr 给出, 说明 u 是 connected, 将用 recv 而不是 recv_from,
/// 以及用 send 而不是 send_to
///
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
    let mut ac = AddrConn::new(Box::new(r), Box::new(w));
    ac.cached_name = String::from("tproxy_udp");
    ac
}

pub struct Writer {
    w: Sender<(Vec<u8>, SocketAddr, SocketAddr)>,
    src: std::net::SocketAddr,
    dst: std::net::SocketAddr,
    conn_map: ConnMap,
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
        if let Ok(mut map) = self.conn_map.try_lock() {
            if let Some(conn_info) = map.get_mut(&(self.dst, self.src)) {
                conn_info.last_active = Instant::now();
            }
        }

        let r = self
            .w
            .try_send((buf.to_vec(), dst.get_socket_addr().unwrap(), self.src));
        match r {
            Ok(_) => Poll::Ready(Ok(buf.len())),
            Err(e) => Poll::Ready(Err(io::Error::other(e))),
        }
    }

    fn poll_close_addr(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let lock_future = self.conn_map.lock();

        match std::pin::pin!(lock_future).poll(cx) {
            Poll::Ready(mut map) => {
                map.remove(&(self.dst, self.src));

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
    rx: Receiver<Vec<u8>>,
    dst: std::net::SocketAddr,
    last_buf: Option<Vec<u8>>,
    state: ReadState,
}
impl ruci::Name for Reader {
    fn name(&self) -> &str {
        "tproxy_udp_r"
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
                    if let Some(b) = self.last_buf.take() {
                        let r_len = b.len();

                        let min_l = min(r_len, buf.len());

                        buf[..min_l].copy_from_slice(b.as_slice());

                        // b.copy_to_slice(&mut buf[..min_l]);

                        if b.is_empty() {
                            self.state = ReadState::Rx;
                        } else {
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
