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

use bytes::{Buf, BytesMut};
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
                    addr: IpAddress::Ipv4(i.into()),
                    port: so.port(),
                },
                std::net::IpAddr::V6(i) => IpEndpoint {
                    addr: IpAddress::Ipv6(i.into()),
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
            addr: ruci::net::NetAddr::Socket(SocketAddr::V4(SocketAddrV4::new(i.into(), a.port))),
        },
        IpAddress::Ipv6(i) => Addr {
            network: ruci::net::Network::UDP,
            addr: ruci::net::NetAddr::Socket(SocketAddr::V6(SocketAddrV6::new(
                i.into(),
                a.port,
                0,
                0,
            ))),
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

        if ready!(me.tx.poll_reserve(cx)).is_ok() {
            if let Err(err) = me.tx.send_item((me.h, ipe, buf.into())) {
                tracing::warn!("tcp send response failed: {}", err);
                Poll::Ready(Err(io::Error::other(err)))
            } else {
                Poll::Ready(Ok(buf.len()))
            }
        } else {
            Poll::Pending
        }
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
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
                let bl = buf.len();
                data.copy_to_slice(&mut buf[..min(data.len(), bl)]);
                Poll::Ready(Ok((data.len(), ip_end_point_to_addr(&src))))
            }
            None => Poll::Ready(Ok((0, Addr::default()))),
        }
    }
}
