use std::{
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
use crate::net::{
    self,
    addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr},
    *,
};

/// socks5 udp conn
#[derive(Clone)]
pub struct Conn {
    base: Arc<UdpSocket>,
    peer_soa: SocketAddr,
}

impl Conn {
    pub fn new(u: UdpSocket, user_soa: SocketAddr) -> Self {
        Conn::newa(Arc::new(u), user_soa)
    }

    pub fn newa(u: Arc<UdpSocket>, user_soa: SocketAddr) -> Self {
        Conn {
            base: u,
            peer_soa: user_soa,
        }
    }
}

pub fn new_addr_conn(u: UdpSocket, user_soa: SocketAddr) -> AddrConn {
    let a = Box::new(Conn::new(u, user_soa));
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
                    net::Addr {
                        addr: net::NetAddr::Socket(so),
                        network: net::Network::UDP,
                    },
                    &mut bf,
                );
                bf.extend_from_slice(buf);

                self.base.poll_send(cx, &mut bf)
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

            Poll::Ready(r) => match r {
                Err(e) => Poll::Ready(Err(e)),
                Ok(so) => {
                    if !so.eq(&self.peer_soa) {
                        // 读到不来自peer的信息时不报错, 直接舍弃
                        info!("socks5 udp got msg not from peer");
                        return Poll::Pending;
                    }
                    let bs = rbuf.filled();

                    let r = decode_read(bs);

                    match r {
                        Err(e) => Poll::Ready(Err(io::Error::other(e.to_string()))),

                        Ok((mut actual_buf, soa)) => {
                            actual_buf.copy_to_slice(buf);

                            Poll::Ready(Ok::<(usize, net::Addr), io::Error>((
                                actual_buf.len(),
                                crate::net::Addr {
                                    addr: NetAddr::Socket(soa),
                                    network: Network::UDP,
                                },
                            )))
                        }
                    }
                }
            },
        }
    }
}
