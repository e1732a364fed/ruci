use std::{
    collections::HashMap,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::{Buf, BytesMut};
use tokio::{
    net::UdpSocket,
    sync::mpsc::{self, Receiver, Sender},
};
use tracing::debug;

use crate::utils::io_error;

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
    rx: mpsc::Receiver<(AddrConn, Addr, BytesMut)>,
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
            let mut hs: HashMap<Addr, Sender<BytesMut>> = HashMap::new();
            loop {
                let r = udp.recv_from(&mut buf).await;
                let (u, a) = match r {
                    Ok(r) => r,
                    Err(_) => break,
                };
                let srt = Addr {
                    network: Network::UDP,
                    addr: NetAddr::Socket(a),
                };

                if hs.contains_key(&srt) {
                    let tx = hs.get(&srt).unwrap();
                    let new_buf = BytesMut::from(&buf[..u]);
                    let r = tx.send(new_buf).await;
                    if let Err(e) = r {
                        debug!("UdpListener tx send got e: {e}");
                        continue;
                    }
                } else {
                    let (tx2, rx2) = mpsc::channel(100);
                    let ac = new(udp.clone(), rx2, srt.clone(), dst_c.clone());

                    hs.insert(srt.clone(), tx2);
                    let new_buf = BytesMut::from(&buf[..u]);
                    let r = tx.send((ac, srt, new_buf)).await;
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
    pub async fn accept(&mut self) -> anyhow::Result<(AddrConn, Addr, Addr, BytesMut)> {
        let (ac, raddr, b) = self
            .rx
            .recv()
            .await
            .ok_or(anyhow::anyhow!("udplistener accept got rx closed"))?;
        Ok((ac, raddr, self.laddr.clone(), b))
    }
}

/// init a AddrConn from a UdpSocket
///
/// 如果 peer_addr 给出, 说明 u 是 connected, 将用 recv 而不是 recv_from,
/// 以及用 send 而不是 send_to
///
pub fn new(u: Arc<UdpSocket>, rx2: Receiver<BytesMut>, dst: Addr, src: Addr) -> AddrConn {
    let r = Reader { dst, rx2 };
    let w = Writer { u: u.clone(), src };
    AddrConn::new(Box::new(r), Box::new(w))
}

pub struct Writer {
    u: Arc<UdpSocket>,
    src: Addr,
}
impl crate::Name for Writer {
    fn name(&self) -> &str {
        "udp"
    }
}
impl AsyncWriteAddr for Writer {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        _addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        // debug!(
        //     "udp write called {} {addr} {}",
        //     buf.len(),
        //     self.peer_addr.is_some()
        // );

        let sor = self.src.get_socket_addr_or_resolve();
        match sor {
            Ok(so) => self.u.poll_send_to(cx, buf, so),
            Err(e) => Poll::Ready(Err(io::Error::other(e))),
        }
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

pub struct Reader {
    rx2: Receiver<BytesMut>,
    dst: Addr,
}
impl crate::Name for Reader {
    fn name(&self) -> &str {
        "udp"
    }
}

impl AsyncReadAddr for Reader {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let r = self.rx2.poll_recv(cx);
        match r {
            Poll::Ready(r) => match r {
                Some(mut b) => {
                    let r_len = b.len();
                    //debug!("udp read got {} {so}", r_len);
                    b.copy_to_slice(buf);

                    Poll::Ready(Ok((r_len, self.dst.clone())))
                }
                None => Poll::Ready(Err(io_error("closed"))),
            },
            Poll::Pending => Poll::Pending,
        }
    }
}
