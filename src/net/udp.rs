/*!
 * 为 UdpSocket 实现 net::addr_conn 中的trait

*/
use super::addr_conn::{AsyncReadAddr, AsyncWriteAddr};
use super::*;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{io::ReadBuf, net::UdpSocket};

pub struct Conn {
    //固定用同一个 udpsocket 发送，到不同的远程地址也是如此
    pub base: Arc<UdpSocket>,
}

pub fn duplicate(u: UdpSocket) -> (Conn, Conn) {
    let a = Arc::new(u);
    let b = a.clone();
    (Conn { base: a }, Conn { base: b })
}

impl AsyncWriteAddr for Conn {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let sor = addr.get_socket_addr_or_resolve();
        match sor {
            Ok(so) => self.base.poll_send_to(cx, buf, so),
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncReadAddr for Conn {
    fn poll_read_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let mut rbuf = ReadBuf::new(buf);
        let r = self.base.poll_recv_from(cx, &mut rbuf);
        match r {
            Poll::Ready(r) => match r {
                Ok(so) => Poll::Ready(Ok((
                    rbuf.filled().len(),
                    crate::net::Addr {
                        addr: NetAddr::Socket(so),
                        network: Network::UDP,
                    },
                ))),
                Err(e) => Poll::Ready(Err(e)),
            },
            Poll::Pending => Poll::Pending,
        }
    }
}

#[allow(unused)]
mod test {
    use udp::addr_conn::{AddrReadTrait, AsyncReadAddrExt};

    use super::*;
    use std::io;

    async fn t1() -> io::Result<()> {
        let u = UdpSocket::bind("127.0.0.1:0").await?;

        unimplemented!()
    }
}
