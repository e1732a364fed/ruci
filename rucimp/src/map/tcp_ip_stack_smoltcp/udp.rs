/*!
Defines a [`new`] function to create an AddrConn that consists of channel based [`W`] and [`R`].
*/

use std::{
    cmp::min,
    io,
    net::{SocketAddr, SocketAddrV4, SocketAddrV6},
    pin::Pin,
    task::{ready, Context, Poll},
};

use bytes::BytesMut;
use ruci::net::{
    addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr},
    Addr,
};
use smoltcp::{
    iface::SocketHandle,
    wire::{IpAddress, IpEndpoint},
};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio_util::sync::PollSender;

#[derive(Clone)]
pub struct W {
    tx: PollSender<(SocketHandle, IpEndpoint, BytesMut)>,
    h: SocketHandle,
    local: Addr,
}
pub struct R {
    rx: Receiver<(IpEndpoint, BytesMut)>,
}

impl ruci::Name for R {
    fn name(&self) -> &str {
        "smoltcp_udp(r)"
    }
}

impl ruci::Name for W {
    fn name(&self) -> &str {
        "smoltcp_udp(w)"
    }
}

/// used by [`super::SmoltcpDevice`]
pub fn new(
    local: Addr,
    h: SocketHandle,
    rx: Receiver<(IpEndpoint, BytesMut)>,
    tx: Sender<(SocketHandle, IpEndpoint, BytesMut)>,
) -> AddrConn {
    let c1 = R { rx };
    let c2 = W {
        tx: PollSender::new(tx),
        h,
        local,
    };
    let mut ac = AddrConn::new(Box::new(c1), Box::new(c2));
    ac.cached_name = "smoltcp_udp".to_string();
    ac
}

fn addr2_ip_end_point(a: &Addr) -> IpEndpoint {
    match a.addr {
        ruci::net::NetAddr::Socket(so) => {
            let ip = so.ip();
            match ip {
                std::net::IpAddr::V4(i) => IpEndpoint {
                    addr: IpAddress::Ipv4(i),
                    port: so.port(),
                },
                std::net::IpAddr::V6(i) => IpEndpoint {
                    addr: IpAddress::Ipv6(i),
                    port: so.port(),
                },
            }
        }
        ruci::net::NetAddr::Name(_, _) => todo!(),
        ruci::net::NetAddr::NameAndSocket(_, _, _) => todo!(),
    }
}

fn ip_end_point_to_addr(a: &IpEndpoint) -> Addr {
    match a.addr {
        IpAddress::Ipv4(i) => Addr {
            network: ruci::net::Network::UDP,
            addr: ruci::net::NetAddr::Socket(SocketAddr::V4(SocketAddrV4::new(i, a.port))),
        },
        IpAddress::Ipv6(i) => Addr {
            network: ruci::net::Network::UDP,
            addr: ruci::net::NetAddr::Socket(SocketAddr::V6(SocketAddrV6::new(i, a.port, 0, 0))),
        },
    }
}

impl AsyncWriteAddr for W {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        _addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let ipe = addr2_ip_end_point(&self.local);
        let me = self.get_mut();

        match ready!(me.tx.poll_reserve(cx)) {
            Ok(_) => match me.tx.send_item((me.h, ipe, buf.into())) {
                Ok(()) => Poll::Ready(Ok(buf.len())),
                Err(e) => {
                    tracing::warn!("UDP send failed: {}", e);
                    Poll::Ready(Err(io::Error::other(e)))
                }
            },
            Err(e) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                format!("channel closed: {}", e),
            ))),
        }
    }
}

impl AsyncReadAddr for R {
    fn poll_read_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let me = self.get_mut();

        match ready!(me.rx.poll_recv(cx)) {
            Some((src, mut data)) => {
                let n = min(data.len(), buf.len());
                let chunk = data.split_to(n);
                buf[..n].copy_from_slice(&chunk);
                Poll::Ready(Ok((n, ip_end_point_to_addr(&src))))
            }
            None => {
                me.rx.close();
                Poll::Ready(Ok((0, Addr::default())))
            }
        }
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let me = self.get_mut();
        me.rx.close();
        Poll::Ready(Ok(()))
    }
}

impl Drop for R {
    fn drop(&mut self) {
        self.rx.close();
    }
}

impl Drop for W {
    fn drop(&mut self) {
        self.tx.close();
    }
}
