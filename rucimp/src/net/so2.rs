/*!
Provides some facilities to configure sockopt using packege `socket2`.
 */

use std::{
    net::{Ipv4Addr, SocketAddrV4},
    time::Duration,
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

use ruci::net::{self, Network, Stream};
use socket2::{Domain, Protocol, Socket, Type};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SockOpt {
    pub tproxy: Option<bool>,
    pub so_mark: Option<u8>,
    pub bind_to_device: Option<String>,
}

/// can listen tcp or dial udp, regard to na.network
///
/// will set non_blocking for all conditions other than udp listen
///
pub fn new_socket2(na: &net::Addr, sopt: &SockOpt, is_listen: bool) -> anyhow::Result<Socket> {
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

    let so = Socket::new(domain, typ, Some(protocol))?;

    #[cfg(target_os = "linux")]
    {
        if sopt.tproxy.unwrap_or_default() {
            super::so_opts::set_tproxy_socket_opts(is_v4, is_udp, &so)?;
        }
        if let Some(m) = sopt.so_mark {
            super::so_opts::set_mark(&so, m)?;
        }
    }

    if let Some(d) = &sopt.bind_to_device {
        #[cfg(target_os = "linux")]
        so.bind_device(Some(d.as_bytes()))?;

        #[cfg(target_os = "macos")]
        {
            // improved from (MIT) shadowsocks-rust
            use core::mem;
            use std::cell::RefCell;
            use std::collections::HashMap;
            use std::io;
            use std::io::ErrorKind;
            use std::os::fd::AsRawFd;
            use std::ptr;
            use tokio::time::Instant;
            use tracing::error;
            fn find_interface_index_cached(iface: &str) -> io::Result<u32> {
                const INDEX_EXPIRE_DURATION: Duration = Duration::from_secs(5);

                thread_local! {
                    static INTERFACE_INDEX_CACHE: RefCell<HashMap<String, (u32, Instant)>> =
                        RefCell::new(HashMap::new());
                }

                let cache_index =
                    INTERFACE_INDEX_CACHE.with(|cache| cache.borrow().get(iface).cloned());
                if let Some((idx, insert_time)) = cache_index {
                    // short-path, cache hit for most cases
                    let now = Instant::now();
                    if now - insert_time < INDEX_EXPIRE_DURATION {
                        return Ok(idx);
                    }
                }

                let index = unsafe {
                    let mut ciface = [0u8; libc::IFNAMSIZ];
                    if iface.len() >= ciface.len() {
                        return Err(ErrorKind::InvalidInput.into());
                    }

                    let iface_bytes = iface.as_bytes();
                    ptr::copy_nonoverlapping(
                        iface_bytes.as_ptr(),
                        ciface.as_mut_ptr(),
                        iface_bytes.len(),
                    );

                    libc::if_nametoindex(ciface.as_ptr() as *const libc::c_char)
                };

                if index == 0 {
                    let err = io::Error::last_os_error();
                    error!("if_nametoindex ifname: {} error: {}", iface, err);
                    return Err(err);
                }

                INTERFACE_INDEX_CACHE.with(|cache| {
                    cache
                        .borrow_mut()
                        .insert(iface.to_owned(), (index, Instant::now()));
                });

                Ok(index)
            }

            fn set_ip_bound_if<S: AsRawFd>(socket: &S, is_4: bool, iface: &str) -> io::Result<()> {
                const IP_BOUND_IF: libc::c_int = 25; // bsd/netinet/in.h
                const IPV6_BOUND_IF: libc::c_int = 125; // bsd/netinet6/in6.h

                unsafe {
                    let index = find_interface_index_cached(iface)?;

                    let ret = match is_4 {
                        true => libc::setsockopt(
                            socket.as_raw_fd(),
                            libc::IPPROTO_IP,
                            IP_BOUND_IF,
                            &index as *const _ as *const _,
                            mem::size_of_val(&index) as libc::socklen_t,
                        ),
                        false => libc::setsockopt(
                            socket.as_raw_fd(),
                            libc::IPPROTO_IPV6,
                            IPV6_BOUND_IF,
                            &index as *const _ as *const _,
                            mem::size_of_val(&index) as libc::socklen_t,
                        ),
                    };

                    if ret < 0 {
                        let err = io::Error::last_os_error();
                        error!(
                            "set IF_BOUND_IF/IPV6_BOUND_IF ifname: {} ifindex: {} error: {}",
                            iface, index, err
                        );
                        return Err(err);
                    }
                }

                Ok(())
            }

            set_ip_bound_if(&so, is_v4, d)?;
        }

        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawSocket;

            let handle = so.as_raw_socket() as SOCKET;

            // improved from (MIT) shadowsocks-rust

            use bytes::BytesMut;
            use windows_sys::Win32::NetworkManagement::IpHelper::*;
            use windows_sys::Win32::Networking::WinSock::*;
            fn find_adapter_interface_index(
                is_4: bool,
                iface: &str,
            ) -> std::io::Result<Option<u32>> {
                // https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getadaptersaddresses

                unsafe {
                    let mut ip_adapter_addresses_buffer = BytesMut::with_capacity(15 * 1024);
                    ip_adapter_addresses_buffer.set_len(15 * 1024);

                    let mut ip_adapter_addresses_buffer_size: u32 =
                        ip_adapter_addresses_buffer.len() as u32;
                    loop {
                        let ret =
                            windows_sys::Win32::NetworkManagement::IpHelper::GetAdaptersAddresses(
                                AF_UNSPEC as u32,
                                GAA_FLAG_SKIP_UNICAST
                                    | GAA_FLAG_SKIP_ANYCAST
                                    | GAA_FLAG_SKIP_MULTICAST
                                    | GAA_FLAG_SKIP_DNS_SERVER,
                                std::ptr::null(),
                                ip_adapter_addresses_buffer.as_mut_ptr() as *mut _,
                                &mut ip_adapter_addresses_buffer_size as *mut _,
                            );

                        match ret {
                            windows_sys::Win32::Foundation::ERROR_SUCCESS => break,
                            windows_sys::Win32::Foundation::ERROR_BUFFER_OVERFLOW => {
                                // resize buffer to ip_adapter_addresses_buffer_size
                                ip_adapter_addresses_buffer
                                    .resize(ip_adapter_addresses_buffer_size as usize, 0);
                                continue;
                            }
                            windows_sys::Win32::Foundation::ERROR_NO_DATA => return Ok(None),
                            _ => {
                                let err = std::io::Error::new(
                                    std::io::ErrorKind::Other,
                                    format!("GetAdaptersAddresses failed with error: {}", ret),
                                );
                                return Err(err);
                            }
                        }
                    }

                    // IP_ADAPTER_ADDRESSES_LH is a linked-list
                    let mut current_ip_adapter_address: *mut IP_ADAPTER_ADDRESSES_LH =
                        ip_adapter_addresses_buffer.as_mut_ptr() as *mut _;
                    while !current_ip_adapter_address.is_null() {
                        let ip_adapter_address: &IP_ADAPTER_ADDRESSES_LH =
                            &*current_ip_adapter_address;

                        use std::os::windows::ffi::OsStringExt;
                        // Friendly Name
                        let friendly_name_len: usize =
                            libc::wcslen(ip_adapter_address.FriendlyName);
                        let friendly_name_slice: &[u16] = std::slice::from_raw_parts(
                            ip_adapter_address.FriendlyName,
                            friendly_name_len,
                        );
                        let friendly_name_os = std::ffi::OsString::from_wide(friendly_name_slice); // UTF-16 to UTF-8
                        if let Some(friendly_name) = friendly_name_os.to_str() {
                            if friendly_name == iface {
                                match is_4 {
                                    true => {
                                        return Ok(Some(
                                            ip_adapter_address.Anonymous1.Anonymous.IfIndex,
                                        ))
                                    }
                                    false => return Ok(Some(ip_adapter_address.Ipv6IfIndex)),
                                }
                            }
                        }

                        // Adapter Name
                        let adapter_name = std::ffi::CStr::from_ptr(
                            ip_adapter_address.AdapterName as *mut _ as *const _,
                        );
                        if adapter_name.to_bytes() == iface.as_bytes() {
                            match is_4 {
                                true => {
                                    return Ok(Some(
                                        ip_adapter_address.Anonymous1.Anonymous.IfIndex,
                                    ))
                                }
                                false => return Ok(Some(ip_adapter_address.Ipv6IfIndex)),
                            }
                        }

                        current_ip_adapter_address = ip_adapter_address.Next;
                    }
                }

                Ok(None)
            }

            fn find_interface_index_cached(is_4: bool, iface: &str) -> std::io::Result<u32> {
                const INDEX_EXPIRE_DURATION: Duration = Duration::from_secs(5);

                thread_local! {
                    static INTERFACE_INDEX_CACHE: std::cell::RefCell<std::collections::HashMap<String, (u32, tokio::time::Instant)>> =
                    std::cell::RefCell::new(std::collections::HashMap::new());
                }

                let cache_index =
                    INTERFACE_INDEX_CACHE.with(|cache| cache.borrow().get(iface).cloned());
                if let Some((idx, insert_time)) = cache_index {
                    // short-path, cache hit for most cases
                    let now = tokio::time::Instant::now();
                    if now - insert_time < INDEX_EXPIRE_DURATION {
                        return Ok(idx);
                    }
                }

                // Get from API GetAdaptersAddresses
                let idx = match find_adapter_interface_index(is_4, iface)? {
                    Some(idx) => idx,
                    None => unsafe {
                        // Windows if_nametoindex requires a C-string for interface name
                        let ifname = std::ffi::CString::new(iface).expect("iface");

                        // https://docs.microsoft.com/en-us/previous-versions/windows/hardware/drivers/ff553788(v=vs.85)
                        let if_index = if_nametoindex(ifname.as_ptr() as windows_sys::core::PCSTR);
                        if if_index == 0 {
                            // If the if_nametoindex function fails and returns zero, it is not possible to determine an error code.
                            tracing::error!("if_nametoindex {} fails", iface);
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "invalid interface name",
                            ));
                        }

                        if_index
                    },
                };

                INTERFACE_INDEX_CACHE.with(|cache| {
                    cache
                        .borrow_mut()
                        .insert(iface.to_owned(), (idx, tokio::time::Instant::now()));
                });

                Ok(idx)
            }
            let if_index = find_interface_index_cached(is_v4, d)?;

            unsafe {
                let if_index = windows_sys::Win32::Networking::WinSock::htonl(if_index);

                if is_v4 {
                    setsockopt(
                        handle,
                        IPPROTO_IP,
                        IP_UNICAST_IF,
                        &if_index as *const _ as windows_sys::core::PCSTR,
                        std::mem::size_of_val(&if_index) as i32,
                    );
                } else {
                    setsockopt(
                        handle,
                        IPPROTO_IPV6,
                        IPV6_UNICAST_IF,
                        &if_index as *const _ as windows_sys::core::PCSTR,
                        std::mem::size_of_val(&if_index) as i32,
                    );
                }
            }
        }
    }
    if is_listen {
        if na.network == Network::TCP {
            so.set_nonblocking(true)?; // NECESSARY
        }
    } else if na.network == Network::UDP {
        so.set_nonblocking(true)?; // NECESSARY!, or it will block the program
    }

    so.set_reuse_address(true)?;

    if is_listen {
        so.bind(&a.into())?;

        if na.network == Network::TCP {
            so.listen(128)?;
        }
    } else {
        let zeroa = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
        so.bind(&zeroa.into()).context("bind failed")?;

        if na.network == Network::TCP {
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!("so2 connecting tcp {}", a);
            }
            so.connect_timeout(&a.into(), Duration::from_secs(3))
                .context("so2 tcp connect failed")?;

            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!("so2 connected tcp {}", a);
            }
            so.set_nonblocking(true)?;

            // 至此, 总结:
            // tcp dial 要设为 nonblocking, udp dial 要设为 nonblocking
            // tcp listen 要设为 nonblocking, udp listen 不要设为 nonblocking (用于tproxy)
        }
    }

    Ok(so)
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

pub fn block_listen_udp_socket(na: &net::Addr, so: &SockOpt) -> anyhow::Result<Socket> {
    let socket = new_socket2(na, so, true)?;

    Ok(socket)
}

#[cfg(target_os = "linux")]
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
    super::so_opts::set_tproxy_socket_opts(is_v4, false, &socket)?;
    // if let Some(m) = so.so_mark {
    //     so_opts::set_mark(&socket, m)?;
    // }
    // if let Some(d) = &so.bind_to_device {
    //     socket.bind_device(Some(d.as_bytes()))?;
    // }

    socket.bind(&laddr.into())?;

    Ok(socket)
}

/// 伪装成 laddr 对 raddr 发数据.
///
/// bind to laddr
///
#[cfg(target_os = "linux")]
pub fn connect_tproxy_udp(laddr: &net::Addr, raddr: &net::Addr) -> anyhow::Result<Socket> {
    let socket = new_socket2_udp_tproxy_dial(laddr)?;
    let ra = raddr
        .get_socket_addr()
        .context("connect_tproxy_udp failed, requires raddr has socket addr")?;

    socket
        .connect_timeout(&ra.into(), Duration::from_millis(200))
        .context("connect_tproxy_udp failed timeout")?;

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
    Ok((Stream::Conn(Box::new(tcp_stream)), ra, la))
}
