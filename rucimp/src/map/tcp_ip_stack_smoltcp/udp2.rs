use std::{
    io::{self},
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
}
pub struct R {
    rx: Receiver<(IpEndpoint, BytesMut)>,
}

impl<'a> ruci::Name for R {
    fn name(&self) -> &str {
        "smoltcp_udp(r)"
    }
}

impl<'a> ruci::Name for W {
    fn name(&self) -> &str {
        "smoltcp_udp(w)"
    }
}

pub fn new(
    h: SocketHandle,
    rx: Receiver<(IpEndpoint, BytesMut)>,
    tx: Sender<(SocketHandle, IpEndpoint, BytesMut)>,
) -> AddrConn {
    let c1 = R { rx };
    let c2 = W {
        tx: PollSender::new(tx),
        h,
    };
    AddrConn::new(Box::new(c1), Box::new(c2))
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

impl<'a> AsyncWriteAddr for W {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let me = self.get_mut();

        let ipe = addr2_ip_end_point(addr);
        if ready!(me.tx.poll_reserve(cx)).is_ok() {
            if let Err(err) = me.tx.send_item((me.h, ipe, buf.into())) {
                tracing::warn!("tcp send response failed: {}", err);
            } else {
                return Poll::Ready(Ok(buf.len()));
            }
        }
        Poll::Ready(Ok(0))
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl<'a> AsyncReadAddr for R {
    fn poll_read_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let me = self.get_mut();
        if let Some((src, mut data)) = ready!(me.rx.poll_recv(cx)) {
            data.copy_to_slice(buf);
            Poll::Ready(Ok((data.len(), ip_end_point_to_addr(&src))))
        } else {
            Poll::Pending
        }
    }
}
