use std::net::{Ipv4Addr, SocketAddrV4};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

use ruci::net::{self, Network, Stream};
use socket2::{Domain, Protocol, Socket, Type};

use super::so_opts;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SockOpt {
    pub tproxy: Option<bool>,
    pub so_mark: Option<u8>,
    pub bind_to_device: Option<String>,
}

/// can listen tcp or dial udp, regard to na.network
///
pub fn new_socket2(na: &net::Addr, so: &SockOpt, is_listen: bool) -> anyhow::Result<Socket> {
    let a = na
        .get_socket_addr()
        .context("new_socket2 failed, requires a has socket addr")?;

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

    if so.tproxy.unwrap_or_default() {
        //debug!("calls set_tproxy_socket_opts");
        so_opts::set_tproxy_socket_opts(is_v4, is_udp, &socket)?;
    }
    if let Some(m) = so.so_mark {
        so_opts::set_mark(&socket, m)?;
    }
    if let Some(d) = &so.bind_to_device {
        socket.bind_device(Some(d.as_bytes()))?;
    }
    if is_listen {
        if na.network == Network::TCP {
            socket.set_nonblocking(true)?;
        }

        // can't set_nonblocking for dial, or we will get
        // Operation now in progress (os error 115) when calling connect
    }
    socket.set_reuse_address(true)?;

    if is_listen {
        socket.bind(&a.into())?;

        if na.network == Network::TCP {
            socket.listen(128)?;
        }
    } else {
        let zeroa = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
        socket.bind(&zeroa.into()).context("bind failed")?;

        if na.network == Network::TCP {
            socket.connect(&a.into()).context("connect failed")?;
        }
    }

    Ok(socket)
}

pub fn listen_tcp(na: &net::Addr, so: &SockOpt) -> anyhow::Result<TcpListener> {
    let socket = new_socket2(na, so, true)?;
    let listener: TcpListener = TcpListener::from_std(std::net::TcpListener::from(socket))?;
    Ok(listener)
}

pub fn dial_tcp(na: &net::Addr, so: &SockOpt) -> anyhow::Result<TcpStream> {
    let socket = new_socket2(na, so, false)?;
    let s: TcpStream = TcpStream::from_std(std::net::TcpStream::from(socket))?;
    Ok(s)
}

/// just bind to empty addr
pub fn dial_udp(na: &net::Addr, so: &SockOpt) -> anyhow::Result<UdpSocket> {
    let socket = new_socket2(na, so, false)?;
    let s: UdpSocket = UdpSocket::from_std(std::net::UdpSocket::from(socket))?;
    Ok(s)
}

pub fn rawlisten_udp(na: &net::Addr, so: &SockOpt) -> anyhow::Result<Socket> {
    //debug!("listen_udp {}", na);
    let socket = new_socket2(na, so, true)?;

    Ok(socket)
}

/// bind to na
pub fn listen_udp(na: &net::Addr, so: &SockOpt) -> anyhow::Result<UdpSocket> {
    //debug!("listen_udp {}", na);
    let socket = new_socket2(na, so, true)?;
    let s: UdpSocket = UdpSocket::from_std(std::net::UdpSocket::from(socket))?;
    Ok(s)
}

pub fn new_socket2_udp_tproxy_dial(laddr: &net::Addr) -> anyhow::Result<Socket> {
    let laddr = laddr
        .get_socket_addr()
        .context("new_socket2_udp_tproxy_dial failed, requires a has socket addr")?;

    let is_v4;
    let domain = if laddr.is_ipv4() {
        is_v4 = true;
        Domain::IPV4
    } else {
        is_v4 = false;
        Domain::IPV6
    };

    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;

    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    // DO NOT set IP_RECVORIGDSTADDR
    so_opts::set_tproxy_socket_opts(is_v4, false, &socket)?;
    // if let Some(m) = so.so_mark {
    //     so_opts::set_mark(&socket, m)?;
    // }
    // if let Some(d) = &so.bind_to_device {
    //     socket.bind_device(Some(d.as_bytes()))?;
    // }

    socket.bind(&laddr.into())?;

    Ok(socket)
}

/// bind to laddr
pub fn connect_tproxy_udp(laddr: &net::Addr, raddr: &net::Addr) -> anyhow::Result<Socket> {
    let socket = new_socket2_udp_tproxy_dial(laddr)?;
    let ra = raddr
        .get_socket_addr()
        .context("connect_tproxy_udp failed, requires raddr has socket addr")?;

    socket.connect(&ra.into()).context("connect failed")?;

    Ok(socket)
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
