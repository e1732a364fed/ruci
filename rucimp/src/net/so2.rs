use anyhow::Context;
use tokio::net::TcpListener;

use ruci::net::{self, Network, Stream};
use socket2::{Domain, Protocol, Socket, Type};

use super::so_opts;

#[derive(Clone, Debug, Default)]
pub struct SockOpt {
    tproxy: bool,
    so_mark: Option<u8>,
    bind_to_device: Option<String>,
}

/// can listen tcp or dial udp, regard to na.network
///
pub async fn new_socket2(na: &net::Addr, so: &SockOpt) -> anyhow::Result<Socket> {
    let a = na
        .get_socket_addr()
        .context("listen_tcp failed, requires a has socket addr")?;

    let is_udp = na.network == Network::UDP;

    let is_v4;
    let domain = if a.is_ipv4() {
        is_v4 = true;
        Domain::IPV4
    } else {
        is_v4 = false;
        Domain::IPV6
    };

    let (typ, protocol) = if is_udp {
        (Type::DGRAM, Protocol::UDP)
    } else {
        (Type::STREAM, Protocol::TCP)
    };

    let socket = Socket::new(domain, typ, Some(protocol))?;

    if so.tproxy {
        so_opts::set_tproxy_socket_opts(is_v4, is_udp, &socket)?;
    }
    if let Some(m) = so.so_mark {
        so_opts::set_mark(&socket, m)?;
    }
    if let Some(d) = &so.bind_to_device {
        socket.bind_device(Some(d.as_bytes()))?;
    }
    socket.set_nonblocking(true)?;
    socket.set_reuse_address(true)?;

    socket.bind(&a.into())?;

    if na.network == Network::TCP {
        socket.listen(128)?;
    }

    Ok(socket)
}

pub async fn listen_tcp(na: &net::Addr, so: &SockOpt) -> anyhow::Result<TcpListener> {
    let socket = new_socket2(na, so).await?;
    let listener: TcpListener = TcpListener::from_std(std::net::TcpListener::from(socket))?;
    Ok(listener)
}

/// returns stream, raddr, laddr
pub async fn accept_tcp(tcp: &TcpListener) -> anyhow::Result<(Stream, net::Addr, net::Addr)> {
    let (tcp_stream, tcp_soa) = tcp.accept().await?;

    let ra = net::Addr {
        addr: net::NetAddr::Socket(tcp_soa),
        network: net::Network::TCP,
    };

    let la = net::Addr {
        addr: net::NetAddr::Socket(tcp_stream.local_addr()?),
        network: net::Network::TCP,
    };
    return Ok((Stream::Conn(Box::new(tcp_stream)), ra, la));
}
