use std::{
    cmp::min,
    collections::{hash_map::Entry, HashMap},
    io,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::{Buf, BytesMut};
use futures::{channel::oneshot, Future};
use tokio::{
    net::UdpSocket,
    sync::{
        mpsc::{self, Receiver, Sender},
        Mutex,
    },
};
use tracing::{debug, trace};

use super::{
    addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr, MAX_DATAGRAM_SIZE},
    Addr,
};

/// 监听一个 udp 端口, 对 每一个 新 源 udp 端口发来的连接
/// 新建一个 Stream::AddrConn. 用于 udp 端口转发
///
/// 只支持 预定义 target_addr
#[derive(Debug)]
pub struct FixedTargetAddrUDPListener {
    laddr: Addr,
    fixed_target: Addr,
    new_conn_rx: mpsc::Receiver<(AddrConn, SocketAddr)>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl FixedTargetAddrUDPListener {
    pub async fn new(laddr: Addr, fixed_target: Addr) -> anyhow::Result<Self> {
        let bind_so = laddr.get_socket_addr_or_resolve(None).await?;

        let u = UdpSocket::bind(bind_so).await?;
        let udp = Arc::new(u);

        let (new_conn_tx, new_conn_rx) = mpsc::channel(100);

        let dst_c = fixed_target.clone();

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        tokio::spawn(async move {
            let mut buf = BytesMut::zeroed(MAX_DATAGRAM_SIZE);
            let conn_map: Arc<Mutex<HashMap<SocketAddr, Sender<BytesMut>>>> =
                Arc::new(Mutex::new(HashMap::new()));
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx=>{
                        debug!("FixedUdpListener got shutdown, will break");
                        break;
                    }

                    r =udp.recv_from(&mut buf) =>{
                        let (n, a) = match r {
                            Ok(r) => r,
                            Err(e) => {
                                debug!("FixedUdpListener loop recv_from got e, will break: {e}");
                                break;
                            }
                        };

                        // mutex guard
                        let mut mg = conn_map.lock().await;

                        //if mg.contains_key(&a) {
                        match mg.entry(a) {
                            Entry::Occupied(e) => {
                                trace!("FixedUdpListener loop got old conn msg: {a} {n}");

                                let new_buf = BytesMut::from(&buf[..n]);

                                let tx = e.get();
                                let r = tx.send(new_buf).await;
                                if let Err(e) = r {
                                    debug!("FixedUdpListener tx send got e: {e}");
                                    continue;
                                }
                            },
                            Entry::Vacant(e) => {

                                trace!("FixedUdpListener loop got new conn: {a} {n}");
                                let (tx, rx) = mpsc::channel(100);

                                e.insert(tx);

                                let ac = new(
                                    udp.clone(),
                                    rx,
                                    a,
                                    dst_c.clone(),
                                    BytesMut::from(&buf[..n]),
                                    conn_map.clone(),
                                );

                                let r = new_conn_tx.send((ac, a)).await;
                                if let Err(e) = r {
                                    debug!("FixedUdpListener loop got e: {e}");
                                    break;
                                }
                            }
                        }
                    }
                }
            } //loop
        });

        Ok(Self {
            shutdown_tx: Some(shutdown_tx),
            laddr,
            new_conn_rx,
            fixed_target,
        })
    }

    /// AddrConn, raddr(udp), laddr(Listener's local addr)
    pub async fn accept(&mut self) -> anyhow::Result<(AddrConn, SocketAddr, Addr)> {
        let (ac, raddr) = self
            .new_conn_rx
            .recv()
            .await
            .ok_or(anyhow::anyhow!("FiexedUdpListener accept got rx closed"))?;
        Ok((ac, raddr, self.laddr.clone()))
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
    pub fn raddr(&self) -> &Addr {
        &self.fixed_target
    }
}

impl Drop for FixedTargetAddrUDPListener {
    fn drop(&mut self) {
        self.shutdown()
    }
}

/// init a AddrConn from a UdpSocket created by a FixedTargetAddrUDPListener
fn new(
    u: Arc<UdpSocket>,
    r: Receiver<BytesMut>,
    src: SocketAddr,
    dst: Addr,

    first_buf: BytesMut,
    conn_map: Arc<Mutex<HashMap<SocketAddr, Sender<BytesMut>>>>,
) -> AddrConn {
    let r = Reader {
        dst,
        rx: r,
        last_buf: Some(first_buf),
        state: ReadState::Buf,
    };
    let w = Writer {
        u: u.clone(),
        src,
        conn_map,
    };
    let mut ac = AddrConn::new(Box::new(r), Box::new(w));
    ac.cached_name = String::from("udp_fixed");
    ac
}

/// write 时会 舍弃 addr. 且直接向内置的 src:Addr 写入数据
struct Writer {
    u: Arc<UdpSocket>,
    src: SocketAddr,
    conn_map: Arc<Mutex<HashMap<SocketAddr, Sender<BytesMut>>>>,
}
impl crate::Name for Writer {
    fn name(&self) -> &str {
        "udp_fixed_w"
    }
}
impl AsyncWriteAddr for Writer {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        _addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let r = self.u.poll_send_to(cx, buf, self.src);

        trace!("udp_fix,write, {}, {r:?}", buf.len());

        r
    }

    fn poll_close_addr(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let cm = &self.conn_map;
        let f = cm.lock();
        let pr = Future::poll(std::pin::pin!(f), cx);
        match pr {
            Poll::Ready(mut map) => {
                map.remove(&self.src);
                trace!("udp_fixed_w got closed, removed from conn map {}", self.src);

                // 移除 tx 后 (drop了), Reader 端的 rx 也会自动失效

                Poll::Ready(Ok(()))
            }
            Poll::Pending => {
                trace!("udp_fixed_w got closed, pending lock");

                Poll::Pending
            }
        }
    }
}

/// Reader 从 rx 读到 数据后，会返回预设的 dst 作为 其addr
struct Reader {
    rx: Receiver<BytesMut>,
    dst: Addr,
    last_buf: Option<BytesMut>,
    state: ReadState,
}
impl crate::Name for Reader {
    fn name(&self) -> &str {
        "udp_fixed_r"
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
        rbuf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        loop {
            match self.state {
                ReadState::Buf => {
                    if let Some(mut old_b) = self.last_buf.take() {
                        let old_len = old_b.len();

                        let real_read_l = min(old_len, rbuf.len());

                        old_b.copy_to_slice(&mut rbuf[..real_read_l]);

                        if old_b.is_empty() {
                            self.state = ReadState::Rx;
                        } else {
                            self.last_buf = Some(old_b);
                        }

                        trace!("udp_fix,read,Buf, {}", real_read_l);

                        return Poll::Ready(Ok((real_read_l, self.dst.clone())));
                    } else {
                        self.state = ReadState::Rx;
                    }
                }
                ReadState::Rx => {
                    trace!("udp_fix,read,Rx");
                    let r = self.rx.poll_recv(cx);
                    trace!("udp_fix,read,Rx,{r:?}");

                    match r {
                        Poll::Ready(rx) => match rx {
                            Some(b) => {
                                trace!("udp_fix,read,Rx, {}", b.len());

                                //debug!("udp_fixed r read got {}", b.len());
                                self.last_buf = Some(b);
                                self.state = ReadState::Buf;
                            }
                            None => {
                                return Poll::Ready(Err(io::Error::new(
                                    io::ErrorKind::ConnectionAborted,
                                    "udp_fixed_r r closed",
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

#[cfg(test)]
mod test {
    use std::time::Duration;

    use crate::net::addr_conn::{AsyncReadAddrExt, AsyncWriteAddrExt};

    use super::*;
    use futures_util::join;
    #[tokio::test]
    async fn test1() -> anyhow::Result<()> {
        let listener_addr = "127.0.0.1:12345";
        let laddr = Addr::from_addr_str("udp", listener_addr).unwrap();
        let dst = Addr::from_addr_str("udp", "127.0.0.1:23456").unwrap();
        let mut listener = FixedTargetAddrUDPListener::new(laddr.clone(), dst).await?;

        let u1 = UdpSocket::bind("127.0.0.1:11211").await?;

        let mut wbuf = [0u8, 2, 2, 3, 4];
        let mut rbuf = [0u8, 0, 0, 0, 0];

        let wbuf2 = [7u8, 2, 2, 3, 4];

        let f1 = tokio::task::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            u1.send_to(&wbuf, laddr.get_socket_addr().unwrap()).await?;
            u1.send_to(&wbuf, laddr.get_socket_addr().unwrap()).await?;

            let n = u1.recv(&mut wbuf).await?;
            println!("r:, {:?} {:?}", n, &wbuf[..n]);

            Ok::<(), io::Error>(())
        });

        let (mut conn, raddr, laddr) = listener.accept().await?;

        println!("raddr, laddr {},{}", raddr, laddr);
        let n = conn.r.read(&mut rbuf).await?;
        println!("dn, {:?} {:?}", n, rbuf);
        let n = conn.r.read(&mut rbuf).await?;
        println!("dn, {:?} {:?}", n, rbuf);
        let fake_a = Addr::default();
        conn.w.write(&wbuf2, &fake_a).await?;

        let _ = join!(f1);

        Ok(())
    }
}
