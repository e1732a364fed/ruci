use std::{
    cmp::min,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use log::info;
use tokio::{io::ReadBuf, net::UdpSocket};

use self::map::{addr_conn::MAX_DATAGRAM_SIZE, helpers::MAX_LEN_SOCKS5_BYTES};

use super::*;
use crate::{
    net::{
        self,
        addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr},
        *,
    },
    Name,
};

/// socks5 udp conn
#[derive(Clone)]
pub struct Conn {
    base: Arc<UdpSocket>,
    peer_soa: SocketAddr,
}
impl Name for Conn {
    fn name(&self) -> &str {
        "socks5_udp"
    }
}

impl Conn {
    pub fn new(u: UdpSocket, peer_soa: SocketAddr) -> Self {
        Conn::newa(Arc::new(u), peer_soa)
    }

    pub fn newa(u: Arc<UdpSocket>, peer_soa: SocketAddr) -> Self {
        Conn { base: u, peer_soa }
    }
}

pub fn new_addr_conn(u: UdpSocket, peersoa: SocketAddr) -> AddrConn {
    let a = Box::new(Conn::new(u, peersoa));
    let b = a.clone();
    AddrConn::new(a, b)
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
            std::result::Result::Ok(so) => {
                let mut bf = BytesMut::with_capacity(buf.len() + MAX_LEN_SOCKS5_BYTES);

                encode_udp_diagram(
                    &net::Addr {
                        addr: net::NetAddr::Socket(so),
                        network: net::Network::UDP,
                    },
                    &mut bf,
                );
                bf.extend_from_slice(buf);

                self.base.poll_send_to(cx, &mut bf, self.peer_soa)
            }
            Err(e) => Poll::Ready(Err(io::Error::other(e))),
        }
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(std::result::Result::Ok(()))
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(std::result::Result::Ok(()))
    }
}

fn decode_read(bs: &[u8]) -> anyhow::Result<(BytesMut, SocketAddr)> {
    let mut bf = BytesMut::from(bs);

    let a = decode_udp_diagram(&mut bf)?;

    let soa = a.get_socket_addr_or_resolve()?;

    Ok((bf, soa))
}

impl AsyncReadAddr for Conn {
    fn poll_read_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let mut new_buf = BytesMut::zeroed(MAX_DATAGRAM_SIZE);

        let mut rbuf = ReadBuf::new(&mut new_buf);
        let r = self.base.poll_recv_from(cx, &mut rbuf);
        match r {
            Poll::Pending => Poll::Pending,

            Poll::Ready(r) => {
                match r {
                    Err(e) => Poll::Ready(Err(e)),
                    Ok(so) => {
                        if !eq_socket_addr(&so, &self.peer_soa) {
                            // 读到不来自peer的信息时不报错, 直接舍弃
                            info!("socks5 udp got msg not from peer, will ignore discard it. is: {:?}, should be: {:?}", so, self.peer_soa);
                            return Poll::Pending;
                        }

                        let bs = rbuf.filled();

                        let r = decode_read(bs);

                        match r {
                            Err(e) => Poll::Ready(Err(io::Error::other(e.to_string()))),

                            Ok((mut actual_buf, soa)) => {
                                let wlen = min(buf.len(), actual_buf.len());
                                actual_buf.copy_to_slice(&mut buf[..wlen]);

                                // if log_enabled!(log::Level::Debug) {
                                //     debug!("socks5 udp got msg,{wlen} {soa}, {:?}", &buf[..wlen])
                                // }

                                Poll::Ready(Ok::<(usize, net::Addr), io::Error>((
                                    wlen,
                                    crate::net::Addr {
                                        addr: NetAddr::Socket(soa),
                                        network: Network::UDP,
                                    },
                                )))
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod test {

    use bytes::BytesMut;
    use tokio::{net::UdpSocket, sync::mpsc};

    use crate::{
        map::socks5::decode_udp_diagram,
        net::{
            addr_conn::{AsyncReadAddrExt, AsyncWriteAddrExt},
            Addr,
        },
    };

    use super::new_addr_conn;

    #[tokio::test]
    async fn test1() -> anyhow::Result<()> {
        //写两遍，一遍错一遍对，然后在 另一端写一遍

        let u = UdpSocket::bind("127.0.0.1:0").await?;
        let ula = u.local_addr()?;
        println!("binded to , {}", ula);

        let so12345 = Addr::from_ip_addr_str("udp", "127.0.0.1:12345")
            .unwrap()
            .get_socket_addr()
            .unwrap();

        //let so12345c = so12345.clone();

        let mut ac = new_addr_conn(u, so12345);

        let (tx, mut rx) = mpsc::channel(10);

        tokio::spawn(async move {
            let mut buf = BytesMut::zeroed(1500);

            for _ in 0..2 {
                println!("try read");

                let r = ac.r.read(&mut buf).await;
                println!("ok read");
                let x = tx.send(r).await;
                if x.is_err() {
                    break;
                }
            }
            println!("try w2");

            let r =
                ac.w.write(b"dfg", &Addr::from_addr_str("udp", "5.6.7.8:90").unwrap())
                    .await;

            println!("try w2 ok, {:?}", r);

            anyhow::Ok(())
        });

        let nu = UdpSocket::bind("127.0.0.1:12345").await?;
        println!("try send, {}", ula);
        nu.send_to(b"abc", ula).await?;
        println!("ok send");

        let readr = rx.recv().await;

        println!("readr: {:?}", readr);
        assert!(readr.unwrap().is_err());
        let mut buf = BytesMut::with_capacity(100);
        crate::map::socks5::encode_udp_diagram(
            &Addr::from_addr_str("udp", "1.2.3.4:56").unwrap(),
            &mut buf,
        );
        buf.extend_from_slice(b"abc");

        println!("try send2, {}", ula);
        nu.send_to(&buf, ula).await?;
        println!("ok send2");

        let readr = rx.recv().await;

        println!("readr: {:?}", readr);
        assert!(readr.unwrap().is_ok());

        let n = nu.recv(&mut buf).await?;

        buf.truncate(n);
        println!("rok, {n}");

        let ra = decode_udp_diagram(&mut buf);
        assert!(ra.is_ok());
        println!("rok, {:?}. {:?}", ra, buf);

        Ok(())
    }
}
