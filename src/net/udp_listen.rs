use std::{
    collections::HashMap,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::{Buf, BytesMut};
use futures::Future;
use tokio::{
    net::UdpSocket,
    sync::{
        mpsc::{self, Receiver, Sender},
        Mutex,
    },
};
use tracing::debug;

use super::{
    addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr, MAX_DATAGRAM_SIZE},
    Addr, NetAddr, Network,
};

/// 监听一个 udp 端口, 对 每一个 新 源 udp 端口发来的连接
/// 新建一个 Stream::AddrConn. 用于 udp 端口转发
#[derive(Debug)]
pub struct FixedTargetAddrUDPListener {
    pub laddr: Addr,
    pub dst: Addr,
    rx: mpsc::Receiver<(AddrConn, Addr)>,
}

impl FixedTargetAddrUDPListener {
    pub async fn new(laddr: Addr, dst: Addr) -> anyhow::Result<Self> {
        let bind_so = laddr.get_socket_addr_or_resolve()?;

        let u = UdpSocket::bind(bind_so).await?;
        let udp = Arc::new(u);

        let (tx, rx) = mpsc::channel(100);

        let dst_c = dst.clone();

        tokio::spawn(async move {
            let mut buf = BytesMut::zeroed(MAX_DATAGRAM_SIZE);
            let conn_map: Arc<Mutex<HashMap<Addr, Sender<BytesMut>>>> =
                Arc::new(Mutex::new(HashMap::new()));
            loop {
                let r = udp.recv_from(&mut buf).await;
                let (u, a) = match r {
                    Ok(r) => r,
                    Err(e) => {
                        debug!("UdpListener loop recv_from got e will break {e}");
                        break;
                    }
                };
                let src = Addr {
                    network: Network::UDP,
                    addr: NetAddr::Socket(a),
                };
                let mut mg = conn_map.lock().await;

                if mg.contains_key(&src) {
                    //debug!("UdpListener loop got old conn msg: {src} {u}");

                    let new_buf = BytesMut::from(&buf[..u]);

                    let tx = mg.get(&src).unwrap();
                    let r = tx.send(new_buf).await;
                    if let Err(e) = r {
                        debug!("UdpListener tx send got e: {e}");
                        continue;
                    }
                } else {
                    //debug!("UdpListener loop got new conn: {src} {u}");
                    let (tx2, rx2) = mpsc::channel(100);

                    mg.insert(src.clone(), tx2);
                    let first_buf = BytesMut::from(&buf[..u]);

                    let ac = new(
                        udp.clone(),
                        rx2,
                        src.clone(),
                        dst_c.clone(),
                        first_buf,
                        conn_map.clone(),
                    );

                    let r = tx.send((ac, src)).await;
                    if let Err(e) = r {
                        debug!("UdpListener loop got e: {e}");
                        break;
                    }
                }
            } //loop
        });

        Ok(Self { laddr, rx, dst })
    }

    /// conn, raddr, laddr, first_data
    pub async fn accept(&mut self) -> anyhow::Result<(AddrConn, Addr, Addr)> {
        let (ac, raddr) = self
            .rx
            .recv()
            .await
            .ok_or(anyhow::anyhow!("udplistener accept got rx closed"))?;
        Ok((ac, raddr, self.laddr.clone()))
    }
}

/// init a AddrConn from a UdpSocket
///
/// 如果 peer_addr 给出, 说明 u 是 connected, 将用 recv 而不是 recv_from,
/// 以及用 send 而不是 send_to
///
pub fn new(
    u: Arc<UdpSocket>,
    r: Receiver<BytesMut>,
    src: Addr,
    dst: Addr,

    first_buf: BytesMut,
    conn_map: Arc<Mutex<HashMap<Addr, Sender<BytesMut>>>>,
) -> AddrConn {
    let r = Reader {
        dst,
        r,
        first_buf: Some(first_buf),
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

pub struct Writer {
    u: Arc<UdpSocket>,
    src: Addr,
    conn_map: Arc<Mutex<HashMap<Addr, Sender<BytesMut>>>>,
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
        //debug!("udp fixed write called {} {addr} {}", buf.len(), self.src);

        let sor = self.src.get_socket_addr_or_resolve();
        match sor {
            Ok(so) => self.u.poll_send_to(cx, buf, so),
            Err(e) => Poll::Ready(Err(io::Error::other(e))),
        }
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close_addr(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let x = &self.conn_map;
        let f = x.lock();
        let x = Future::poll(Pin::new(&mut Box::pin(f)), cx);
        match x {
            Poll::Ready(mut map) => {
                map.remove(&self.src);
                //debug!("udp_fixed_w got closed, removed from conn map {}", self.src);

                // 移除 tx 后 (drop了), Reader 端的 rx 也会自动失效

                Poll::Ready(Ok(()))
            }
            Poll::Pending => {
                //debug!("udp_fixed_w got closed, pending lock");

                Poll::Pending
            }
        }
    }
}

pub struct Reader {
    r: Receiver<BytesMut>,
    dst: Addr,
    first_buf: Option<BytesMut>,
}
impl crate::Name for Reader {
    fn name(&self) -> &str {
        "udp_fixed_r"
    }
}

impl AsyncReadAddr for Reader {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        if let Some(mut b) = self.first_buf.take() {
            let r_len = b.len();
            //debug!("udp fix read first {} {}", r_len, self.dst);
            b.copy_to_slice(&mut buf[..r_len]);

            return Poll::Ready(Ok((r_len, self.dst.clone())));
        }
        let r = self.r.poll_recv(cx);
        match r {
            Poll::Ready(r) => match r {
                Some(mut b) => {
                    let r_len = b.len();
                    //debug!("udp fix read  got {} ", r_len);
                    b.copy_to_slice(&mut buf[..r_len]);

                    Poll::Ready(Ok((r_len, self.dst.clone())))
                }
                None => Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "udp_fixed_r r closed",
                ))),
            },
            Poll::Pending => Poll::Pending,
        }
    }
}
