/*!
Defines a [`Conn`] that impls traits in [`net::addr_conn`] and holds a [`Socket`].

本模块暂时没有被用到.
*/

use super::addr_conn::{AsyncReadAddr, AsyncWriteAddr};
use super::*;
use ruci::net::addr_conn::AddrConn;
use ruci::utils::io_error;
use smoltcp::phy::PacketMeta;
use smoltcp::socket::udp::{Socket, UdpMetadata};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::sync::Mutex;

/// Implements AddrConn trait
///
/// 固定用同一个 udp socket 发送, 到不同的远程地址也是如此
#[derive(Clone)]
pub struct Conn<'a> {
    u: Arc<Mutex<Socket<'a>>>,
    peer_addr: Option<Addr>,
}
impl<'a> ruci::Name for Conn<'a> {
    fn name(&self) -> &str {
        "smoltcp_udp"
    }
}

impl<'a> Conn<'a> {
    /// init a Conn from a UdpSocket
    ///
    /// 如果 peer_addr 给出, 说明 u 是 connected, 将用 recv 而不是 recv_from,
    /// 以及用 send 而不是 send_to
    ///
    pub fn new(u: Socket<'a>, peer_addr: Option<Addr>) -> Self {
        Conn {
            u: Arc::new(Mutex::new(u)),
            peer_addr,
        }
    }
}

/// init a AddrConn from a UdpSocket
///
/// 如果 peer_addr 给出, 说明 u 是 connected, 将用 recv 而不是 recv_from,
/// 以及用 send 而不是 send_to
///
pub fn new(u: Socket<'static>, peer_addr: Option<Addr>) -> AddrConn {
    let a = Arc::new(Mutex::new(u));
    let b = a.clone();

    let c1 = Conn {
        u: a,
        peer_addr: peer_addr.clone(),
    };
    let c2 = Conn { u: b, peer_addr };
    AddrConn::new(Box::new(c1), Box::new(c2))
}

/// wrap u with Arc, then return 2 copies.
pub fn duplicate(u: Socket) -> (Conn, Conn) {
    let a = Arc::new(Mutex::new(u));
    let b = a.clone();
    (
        Conn {
            u: a,
            peer_addr: None,
        },
        Conn {
            u: b,
            peer_addr: None,
        },
    )
}

impl<'a> AsyncWriteAddr for Conn<'a> {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        //debug!("udp write called {} {addr} {:?}", buf.len(), self.peer_addr);

        let x = futures::Future::poll(std::pin::pin!(self.u.lock()), cx);

        match x {
            Poll::Ready(mut u) => {
                if !u.can_send() {
                    u.register_send_waker(cx.waker());
                    return Poll::Pending;
                };

                let addr = if self.peer_addr.is_some() || addr.eq(&Addr::default()) {
                    match &self.peer_addr {
                        Some(a) => a.get_socket_addr().unwrap(),
                        None => Addr::default().get_socket_addr().unwrap(),
                    }
                } else {
                    let sor = addr.get_socket_addr_or_resolve();
                    match sor {
                        Ok(so) => so,
                        Err(e) => return Poll::Ready(Err(io::Error::other(e))),
                    }
                };
                let ed = smoltcp::wire::IpEndpoint::from(addr);

                let r = u.send_slice(
                    buf,
                    UdpMetadata {
                        endpoint: smoltcp::wire::IpEndpoint::from(ed),
                        meta: PacketMeta::default(),
                    },
                );
                match r {
                    Ok(_) => todo!(),
                    Err(_) => todo!(),
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl<'a> AsyncReadAddr for Conn<'a> {
    fn poll_read_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let x = futures::Future::poll(std::pin::pin!(self.u.lock()), cx);
        match x {
            Poll::Pending => return Poll::Pending,

            Poll::Ready(mut u) => {
                if !u.can_recv() {
                    u.register_recv_waker(cx.waker());
                    return Poll::Pending;
                };
                if let Some(pa) = self.peer_addr.clone().as_ref() {
                    let r = u.recv_slice(buf);
                    match r {
                        Ok((n, _)) => {
                            //debug!("udp with peer_addr read got {}", r_len);

                            Poll::Ready(Ok((n, pa.clone())))
                        }
                        Err(e) => Poll::Ready(Err(io_error(e))),
                    }
                } else {
                    let r = u.recv_slice(buf);
                    match r {
                        Ok((n, md)) => {
                            //debug!("udp with peer_addr read got {}", r_len);
                            let ed = md.endpoint;
                            let ip = match ed.addr {
                                smoltcp::wire::IpAddress::Ipv4(a) => IpAddr::V4(Ipv4Addr::from(a)),
                                smoltcp::wire::IpAddress::Ipv6(a) => IpAddr::V6(Ipv6Addr::from(a)),
                            };
                            let sa = SocketAddr::new(ip, ed.port);
                            let addr = ruci::net::Addr {
                                addr: NetAddr::Socket(sa),
                                network: Network::UDP,
                            };

                            Poll::Ready(Ok((n, addr)))
                        }
                        Err(e) => Poll::Ready(Err(io_error(e))),
                    }
                }
            }
        }
    }
}
