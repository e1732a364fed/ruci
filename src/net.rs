/*!
 * module net defines some important parts for proxy.
 *
 * important parts: Addr, ConnTrait, Conn, TransmissionInfo,
 *  and a cp function for copying data between Conn
 *
*/
pub mod addr_conn;
pub mod helpers;
pub mod udp;

#[cfg(test)]
mod test;

use futures::pin_mut;
use futures::select;
use futures::{io::Error, FutureExt};
use log::{debug, log_enabled};
use rand::Rng;
use std::io;
use std::sync::atomic::AtomicU32;
use std::{fmt::Debug, net::Ipv4Addr};
use std::{
    fmt::{Display, Formatter},
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::Arc,
};
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::net::UdpSocket;
use tokio::net::UnixStream;

pub fn ip_addr_to_u8_vec(ip_addr: IpAddr) -> Vec<u8> {
    match ip_addr {
        IpAddr::V4(v4) => v4.octets().to_vec(),
        IpAddr::V6(v6) => v6.octets().to_vec(),
    }
}

pub fn gen_random_ipv6() -> IpAddr {
    let mut rng = rand::thread_rng();
    let mut octets = [0; 16];
    rng.fill(&mut octets);
    IpAddr::V6(Ipv6Addr::from(octets))
}

/// 1024..=65535
pub fn gen_random_port() -> u16 {
    let mut rng = rand::thread_rng();
    rng.gen_range(1024..=65535)
}

///10240..=65535
pub fn gen_random_higher_port() -> u16 {
    let mut rng = rand::thread_rng();
    rng.gen_range(10240..=65535)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NetAddr {
    Socket(SocketAddr), //ip+port
    Name(String, u16),
    NameAndSocket(String, SocketAddr, u16),
}

impl Default for NetAddr {
    /// ipv4 0.0.0.0:0
    fn default() -> Self {
        NetAddr::Socket(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum Network {
    IP,

    #[default]
    TCP,
    UDP,

    #[cfg(unix)]
    Unix,
}

impl Network {
    pub fn from_string(s: &str) -> io::Result<Self> {
        match s {
            "ip" => Ok(Network::IP),
            "tcp" => Ok(Network::TCP),
            "udp" => Ok(Network::UDP),
            #[cfg(unix)]
            "unix" => Ok(Network::Unix),
            _ => Err(io::Error::other(format!(
                "not supported network string: {}",
                s
            ))),
        }
    }

    pub fn to_static_str(&self) -> &'static str {
        match self {
            Network::IP => "ip",
            Network::TCP => "tcp",
            Network::UDP => "udp",
            #[cfg(unix)]
            Network::Unix => "unix",
        }
    }
}

impl Display for Network {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_static_str())
    }
}

pub enum IPName {
    IP(IpAddr),
    Name(String),
}

/// Addr 结构是一个可表示多种网络地址的结构,
/// 可以是 ip, ipv4, ipv6, unix domain socket, domain name
///
/// 具体是哪一种由 network 决定。若 network 不为 unix，
/// addr 可以为 Socket 或 Name (表示 domain name),
/// 否则 addr 只能为 Name (表示 file name)
///
/// port = 0 表示不用端口
///
/// Addr实现 Eq和 Hash，以支持作为Key存入 HashMap 等集合中。
///
/// default is  tcp://0.0.0.0:0
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct Addr {
    pub addr: NetAddr,
    pub network: Network,
}

impl Addr {
    pub fn from(
        network: &str,
        host: Option<String>,
        ip: Option<IpAddr>,
        port: u16,
    ) -> io::Result<Self> {
        let n = Network::from_string(network)?;
        match host {
            Some(h) => {
                if let Some(ip) = ip {
                    Ok(Addr {
                        addr: NetAddr::NameAndSocket(h, SocketAddr::new(ip, port), port),
                        network: n,
                    })
                } else {
                    let a = NetAddr::Name(h, port);
                    Ok(Addr {
                        addr: a,
                        network: n,
                    })
                }
            }
            None => Ok(Addr {
                addr: NetAddr::Socket(SocketAddr::new(
                    ip.ok_or(io::Error::other("neither of ip or host provided"))?,
                    port,
                )),
                network: n,
            }),
        }
    }

    // convert then calls "from"
    pub fn from_strs(network: &str, host: &str, ip: &str, port: u16) -> io::Result<Self> {
        let mut host_is_ip = false;

        let ip = if ip == "" {
            if host == "" {
                None
            } else {
                let parsed = host.parse::<IpAddr>();
                if let Ok(i) = parsed {
                    host_is_ip = true;
                    Some(i)
                } else {
                    None
                }
            }
        } else {
            ip.parse::<IpAddr>().ok()
        };

        let host = if host == String::new() {
            None
        } else {
            if host_is_ip {
                None
            } else {
                Some(host.to_string())
            }
        };

        Addr::from(network, host, ip, port)
    }

    /// will set network to Tcp
    pub fn from_ipname(ipn: IPName, port: u16) -> Self {
        match ipn {
            IPName::IP(ip) => Addr {
                network: Network::TCP,
                addr: NetAddr::Socket(SocketAddr::new(ip, port)),
            },
            IPName::Name(n) => Addr {
                network: Network::TCP,
                addr: NetAddr::Name(n, port),
            },
        }
    }

    pub fn get_name(&self) -> Option<String> {
        match &self.addr {
            NetAddr::Name(n, _) => Some(n.clone()),
            NetAddr::NameAndSocket(n, _, _) => Some(n.clone()),
            _ => None,
        }
    }
    pub fn get_port(&self) -> u16 {
        match &self.addr {
            NetAddr::Name(_, p) => *p,
            NetAddr::NameAndSocket(_, _, p) => *p,
            NetAddr::Socket(so) => so.port(),
        }
    }

    pub fn get_ip(&self) -> Option<IpAddr> {
        match &self.addr {
            NetAddr::NameAndSocket(_, so, _) => Some(so.ip()),
            NetAddr::Socket(so) => Some(so.ip()),
            _ => None,
        }
    }

    /// 只由 SocketAddr 转，无视 name
    pub fn get_socket_addr(&self) -> Option<SocketAddr> {
        if let NetAddr::Socket(s) = &self.addr {
            Some(*s)
        } else if let NetAddr::NameAndSocket(_, so, _) = &self.addr {
            Some(*so)
        } else {
            None
        }
    }

    /// 如果没法从已有的 SocketAddr 转，则尝试用系统方法解析域名, 并使用第一个值.
    /// 不适用于 UDS
    pub fn get_socket_addr_or_resolve(&self) -> io::Result<SocketAddr> {
        use std::net::ToSocketAddrs;

        if let NetAddr::Socket(s) = self.addr {
            Ok(s)
        } else if let NetAddr::NameAndSocket(_, so, _) = &self.addr {
            Ok(*so)
        } else if let NetAddr::Name(n, port) = &self.addr {
            let so = (format!("{}:{}", n, port)).to_socket_addrs();
            so?.next().ok_or(io::Error::other(format!(
                "resolve to empty socket_addr from {}",
                self
            )))
        } else {
            Err(io::Error::other(format!(
                "not possible convert to socket_addr from {}",
                self
            )))
        }
    }

    pub async fn try_dial(&self) -> io::Result<Stream> {
        match self.network {
            Network::TCP => {
                let so = self.get_socket_addr_or_resolve()?;

                let c = TcpStream::connect(so).await?;
                return Ok(Stream::TCP(Box::new(c)));
            }
            Network::UDP => {
                let so = self.get_socket_addr_or_resolve()?;

                let u = UdpSocket::bind(so).await?;
                return Ok(Stream::UDP(udp::new(u)));
            }
            #[cfg(unix)]
            Network::Unix => {
                let u = UnixStream::connect(self.get_name().unwrap_or_default()).await?;
                return Ok(Stream::TCP(Box::new(u)));
            }
            _ => unimplemented!(),
        }
    }

    ///可为 www.baidu.com:80 或 127.0.0.1:1234 这种形式,
    /// 如果 name和ip都给出了，首选ip
    ///
    /// 如果为 UDS, 则不会打印 port
    pub fn get_addr_str(&self) -> String {
        match self.network {
            Network::IP => match &self.addr {
                NetAddr::Socket(so) => so.to_string(),
                NetAddr::Name(n, _) => n.clone(),
                NetAddr::NameAndSocket(_, ip, _) => ip.to_string(),
            },
            Network::TCP | Network::UDP => match &self.addr {
                NetAddr::Socket(so) => so.to_string(),
                NetAddr::Name(n, p) => format!("{}:{}", n, p),
                NetAddr::NameAndSocket(_, ip, p) => format!("{}:{}", ip, p),
            },
            #[cfg(unix)]
            Network::Unix => match &self.addr {
                NetAddr::Socket(_) => {
                    panic!("network is unix but addr in Addr is SocketAddr rather than Name")
                }
                NetAddr::Name(n, _) | NetAddr::NameAndSocket(n, _, _) => n.to_string(),
            },
        }
    }
}

/// 以 url 的格式 描述 Addr
impl Display for Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.get_addr_str();
        write!(f, "{}://{}", self.network.to_static_str(), s)
    }
}

pub struct OptAddrRef<'a>(pub &'a Option<Addr>);
pub struct OptAddr(pub Option<Addr>);

impl Display for OptAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(value) => write!(f, "{}", value),
            None => write!(f, "EmptyAddr"),
        }
    }
}

impl<'a> Display for OptAddrRef<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(value) => write!(f, "{}", value),
            None => write!(f, "EmptyAddr"),
        }
    }
}

#[derive(Default)]
pub enum Stream {
    ///  tcp / unix domain socket 等 目标 Addr 唯一的 情况
    TCP(Conn),
    UDP(AddrConn),

    // 比如： tcp listener
    Generator(tokio::sync::mpsc::Receiver<Stream>),

    #[default]
    None,
}
impl Stream {
    pub async fn try_shutdown(self) -> io::Result<()> {
        if let Stream::TCP(mut t) = self {
            t.shutdown().await?
        } else if let Stream::UDP(mut c) = self {
            use crate::net::addr_conn::AsyncWriteAddrExt;

            c.1.shutdown().await?
        }
        Ok(())
    }

    pub fn try_unwrap_tcp(self) -> io::Result<Conn> {
        if let Stream::TCP(t) = self {
            return Ok(t);
        }
        Err(io::Error::other("not tcp"))
    }

    pub fn try_unwrap_udp(self) -> io::Result<AddrConn> {
        if let Stream::UDP(t) = self {
            return Ok(t);
        }
        Err(io::Error::other("not tcp"))
    }
}

use std::sync::atomic::{AtomicU64, Ordering};

use crate::Name;

use self::addr_conn::AddrConn;

/// 用于状态监视和流量统计；可以用 Arc<TransmissionInfo> 进行全局的监视和统计。
#[derive(Debug, Default)]
pub struct TransmissionInfo {
    pub last_connection_id: AtomicU32,

    pub alive_connection_count: AtomicU32,

    /// total downloaded bytes since start
    pub db: AtomicU64,

    /// total uploaded bytes
    pub ub: AtomicU64,
}

/// ConnTrait 将 可异步读写的功能抽象出来。TcpStream 也实现了 ConnTrait
/// 这是一个很重要的 Trait
pub trait ConnTrait: AsyncRead + AsyncWrite + Unpin + Send + Sync + Name {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Sync + Name> ConnTrait for T {}

impl crate::Name for TcpStream {
    fn name(&self) -> &str {
        "tcpstream"
    }
}

impl crate::Name for UnixStream {
    fn name(&self) -> &str {
        "unixstream"
    }
}

/// 一个方便的 对 ConnTrait 的包装。很重要
pub type Conn = Box<dyn ConnTrait>;

/// may log debug or do other side-effect stuff with id.
pub async fn cp<C1: ConnTrait, C2: ConnTrait>(
    c1: C1,
    c2: C2,
    cid: u32,
    opt: Option<Arc<TransmissionInfo>>,
) -> Result<u64, Error> {
    if log_enabled!(log::Level::Debug) {
        debug!("cp start, cid: {} ", cid);
    }

    let (mut c1_read, mut c1_write) = tokio::io::split(c1);
    let (mut c2_read, mut c2_write) = tokio::io::split(c2);

    let (c1_to_c2, c2_to_c1) = (
        tokio::io::copy(&mut c1_read, &mut c2_write).fuse(),
        tokio::io::copy(&mut c2_read, &mut c1_write).fuse(),
    );

    pin_mut!(c1_to_c2, c2_to_c1);

    // 一个方向停止后，关闭连接，如果info 不为空，则等待另一个方向关闭, 以获取另一方向的流量信息。

    select! {
        r1 = c1_to_c2 => {
            if let Some(ref info) = opt {
                if let Ok(n) = r1 {
                    let tt = info.ub.fetch_add(n, Ordering::Relaxed);

                    if log_enabled!(log::Level::Debug) {
                        debug!("cp, cid: {}, u, ub, {}, {}",cid,n,tt+n);
                    }
                }

                // can't borrow mut more than once. We just hope tokio will shutdown tcp
                // when it's dropped.
                // during the tests we can prove it's dropped.

                let r2 = c2_to_c1.await;
                if let Some(info) = opt {
                    if let Ok(n) = r2 {
                        let tt = info.db.fetch_add(n, Ordering::Relaxed);

                        if log_enabled!(log::Level::Debug) {
                            debug!("cp, cid: {}, u, db, {}, {}",cid, n,tt+n);
                        }
                    }
                }
            }


            if log_enabled!(log::Level::Debug) {
                debug!("cp end u, cid: {} ",cid);
            }

            r1
        },
        r2 = c2_to_c1 => {
            if let Some(ref info) = opt {
                if let Ok(n) = r2 {
                    let tt = info.db.fetch_add(n, Ordering::Relaxed);

                    if log_enabled!(log::Level::Debug) {
                        debug!("cp, cid: {}, d, db, {}, {}",cid, n,tt+n);
                    }
                }

                let r1 = c1_to_c2.await;
                if let Some(ref info) = opt {
                    if let Ok(n) = r1 {
                        let tt = info.ub.fetch_add(n, Ordering::Relaxed);

                        if log_enabled!(log::Level::Debug) {
                            debug!("cp, cid: {}, d, ub, {}, {}",cid,n,tt+n);
                        }
                    }
                }
            }

            if log_enabled!(log::Level::Debug) {
                debug!("cp end d, { } ",cid);
            }

            r2
        },
    }
}
