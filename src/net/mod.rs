/*!
 * module net defines some important parts for proxy.
 *
 * important parts: CID, Network, Addr, ConnTrait, Conn, Stream, TrafficRecorder,
 *  and a cp function for copying data between Conn

 enums:
CID, Network ,Addr ,Stream

 structs:
 CIDChain, TrafficRecorder,

trait: ConnTrait

type: Conn

function: cp

*/
pub mod addr_conn;
pub mod helpers;
pub mod http;
pub mod listen;
pub mod udp;

#[cfg(feature = "tun")]
pub mod tun;

#[cfg(test)]
mod test;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use futures::pin_mut;
use futures::{io::Error, FutureExt};
use log::{debug, log_enabled};
use rand::Rng;
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
#[cfg(unix)]
use tokio::net::UnixStream;

// #[derive(Default)]
// pub enum GenerateCIDBehavior {
//     #[default]
//     Random,
//     Ordered,
// }

pub fn new_rand_cid() -> u32 {
    const ID_RANGE_START: u32 = 100_000;

    rand::thread_rng().gen_range(ID_RANGE_START..=ID_RANGE_START * 10 - 1)
}

pub fn new_ordered_cid(lastid: &std::sync::atomic::AtomicU32) -> u32 {
    lastid.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

#[derive(Clone, Debug)]
pub struct CIDChain {
    pub id_list: Vec<u32>, //首项为根id, 末项为末端stream的id
}
impl Display for CIDChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.id_list).len() {
            0 => {
                write!(f, "[ empty ]")
            }
            1 => {
                write!(
                    f,
                    "[ cid: {} ]",
                    self.id_list.first().expect("get first element of cid")
                )
            }
            _ => {
                let v: Vec<_> = self.id_list.iter().map(|id| id.to_string()).collect();
                let s = v.join("-");
                write!(f, "[ cid: {} ]", s)
            }
        }
    }
}

/// stream id ('c' for conn as convention)
#[derive(Clone, Debug)]
pub enum CID {
    Unit(u32),
    Chain(CIDChain),
}

impl CID {
    ///random
    pub fn new() -> CID {
        CID::Unit(new_rand_cid())
    }

    /// new with ordered id
    pub fn new_ordered(lastid: &std::sync::atomic::AtomicU32) -> CID {
        let li = new_ordered_cid(lastid);
        CID::Unit(li)
    }

    pub fn new_by_opti(oti: Option<Arc<TrafficRecorder>>) -> CID {
        match oti {
            Some(ti) => CID::new_ordered(&ti.last_connection_id),
            None => CID::new(),
        }
    }

    ///
    /// # Example
    ///
    /// ```
    /// use ruci::net::CID;
    /// use ruci::net::TrafficRecorder;
    /// use std::sync::Arc;
    /// let oti = Some(Arc::new(TrafficRecorder::default()));
    ///
    /// let mut x = CID::new_by_opti(oti.clone());
    /// assert!(matches!(x, CID::Unit(1)));
    /// x.push(oti.clone());
    /// assert!(matches!(x.clone(), CID::Chain(chain) if chain.id_list[0] == 1));
    ///
    /// x.push(oti);
    /// assert!(
    ///     matches!(x.clone(), CID::Chain(chain) if chain.id_list[0] == 1 || chain.id_list[1] == 2)
    /// );
    ///
    /// ```
    pub fn push(&mut self, oti: Option<Arc<TrafficRecorder>>) {
        let newidnum = match oti.as_ref() {
            Some(ti) => new_ordered_cid(&ti.last_connection_id),
            None => new_rand_cid(),
        };

        match self {
            CID::Unit(u) => {
                let x = *u;
                if x == 0 {
                    *self = CID::Unit(newidnum);
                } else {
                    *self = CID::Chain(CIDChain {
                        id_list: vec![x, newidnum],
                    })
                }
            }
            CID::Chain(c) => c.id_list.push(newidnum),
        };
    }
    pub fn clone_push(&self, oti: Option<Arc<TrafficRecorder>>) -> Self {
        let mut cid = self.clone();
        cid.push(oti);
        cid
    }
}

impl Default for CID {
    fn default() -> Self {
        CID::Unit(0)
    }
}

impl Display for CID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CID::Unit(u) => write!(f, "[ cid: {u} ]"),
            CID::Chain(c) => Display::fmt(c, f),
        }
    }
}

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

/// work better than use a.eq(b).
///
/// it checks if a or b is unspecified (this will be equal too)
///
pub fn eq_socket_addr(a: &SocketAddr, b: &SocketAddr) -> bool {
    if a.eq(b) {
        true
    } else {
        if a.port() != b.port() {
            false
        } else {
            if a.ip().is_unspecified() || b.ip().is_unspecified() {
                true
            } else {
                false
            }
        }
    }
}

/// default: ipv4 0.0.0.0:0
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
    pub fn is_tcp_or_udp(&self) -> bool {
        matches!(self, Network::TCP) || matches!(self, Network::UDP)
    }
    pub fn from_string(s: &str) -> Result<Self> {
        let s = String::from(s).to_lowercase();

        match s.as_str() {
            "ip" => Ok(Network::IP),
            "tcp" => Ok(Network::TCP),
            "udp" => Ok(Network::UDP),
            #[cfg(unix)]
            "unix" => Ok(Network::Unix),
            _ => Err(anyhow!("not supported network string: {}", s)),
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

fn prefix_length_to_netmask(prefix_length: u8) -> (u8, u8, u8, u8) {
    // Ensure the prefix length is valid
    if prefix_length > 32 {
        panic!("Invalid prefix length: {}", prefix_length);
    }

    // Calculate the netmask
    let netmask: u32 = !(0xFFFFFFFF >> prefix_length);

    // Extract the octets from the netmask
    let o1 = (netmask >> 24) & 0xFF;
    let o2 = (netmask >> 16) & 0xFF;
    let o3 = (netmask >> 8) & 0xFF;
    let o4 = netmask & 0xFF;

    (o1 as u8, o2 as u8, o3 as u8, o4 as u8)
}

/// Addr 结构是一个可表示多种网络地址的结构,
/// 可以是 ip, ipv4, ipv6, unix domain socket, domain name
///
/// 具体是哪一种由 network 决定。若 network 不为 unix,
/// addr 可以为 Socket 或 Name (表示 domain name),
/// 否则 addr 只能为 Name (表示 file name)
///
/// port = 0 表示不用端口, 或表示让系统在拨号时使用系统分配的端口
///
/// Addr实现 Eq和 Hash, 以支持作为Key存入 HashMap 等集合中。
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
    ) -> Result<Self> {
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
                    ip.ok_or(anyhow!("neither of ip or host provided"))?,
                    port,
                )),
                network: n,
            }),
        }
    }

    // convert then calls "from"
    pub fn from_strs(network: &str, host: &str, ip: &str, port: u16) -> Result<Self> {
        let mut host_is_ip = false;

        let ip = if ip.is_empty() {
            if host.is_empty() {
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

        let host = if (host == String::new()) || host_is_ip {
            None
        } else {
            Some(host.to_string())
        };

        Addr::from(network, host, ip, port)
    }

    /// like
    ///
    /// ip://10.0.0.1:24#utun321
    ///
    /// tcp://127.0.0.1:80#www.b.com
    ///
    /// if no "#", it will fallback to from_network_addr_str
    ///
    pub fn from_name_network_addr_str(s: &str) -> Result<Self> {
        let ns: Vec<_> = s.splitn(2, "#").collect();
        match ns.len() {
            1 => Addr::from_network_addr_str(s),
            2 => {
                let a = Addr::from_network_addr_str(ns[0])?;
                Ok(a.set_name(ns[1]))
            }
            _ => Err(anyhow!(
                "Addr::from_name_network_addr_str, split # got len!=2 && len!=1",
            )),
        }
    }

    /// tcp://127.0.0.1:80 or tcp://www.b.com:80.
    ///
    /// if :// is not present, use tcp as network, like 1.1.1.1:1 will act like
    /// tcp://1.1.1.1:1
    pub fn from_network_addr_str(s: &str) -> Result<Self> {
        let ns: Vec<_> = s.splitn(2, "://").collect();
        match ns.len() {
            1 => Addr::from_addr_str("tcp", s),
            2 => Addr::from_addr_str(ns[0], ns[1]),
            _ => Err(anyhow!(
                "Addr::from_network_addr_str, split :// got len!=2 && len!=1",
            )),
        }
    }

    /// 127.0.0.1:80 or www.b.com:80. if unix, then like path/to/file, without the port and colon.
    pub fn from_addr_str(network: &str, s: &str) -> Result<Self> {
        let ns: Vec<_> = s.split(':').collect();
        let port = if ns.len() != 2 {
            0
        } else {
            ns[1].parse::<u16>().map_err(|e| anyhow!("{}", e))?
        };

        let x = ns[0].parse::<IpAddr>();
        match x {
            Ok(ip) => Addr::from(network, None, Some(ip), port),
            Err(_) => Addr::from(network, Some(ns[0].to_string()), None, port),
        }
    }

    /// 127.0.0.1:80
    pub fn from_ip_addr_str(network: &'static str, s: &str) -> Result<Self> {
        let ns: Vec<_> = s.split(':').collect();
        if ns.len() != 2 {
            return Err(anyhow!("Addr::from_ip_addr_str, split colon got len!=2",));
        }
        Addr::from_strs(
            network,
            "",
            ns[0],
            ns[1].parse::<u16>().map_err(|e| anyhow!("{}", e))?,
        )
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

    #[cfg(unix)]
    pub fn from_unix(unix_soa: tokio::net::unix::SocketAddr) -> Self {
        if unix_soa.is_unnamed() {
            Addr {
                addr: NetAddr::Name("".to_string(), 0),
                network: Network::Unix,
            }
        } else {
            let p = unix_soa
                .as_pathname()
                .unwrap()
                .to_string_lossy()
                .to_string();

            Addr {
                addr: NetAddr::Name(p, 0),
                network: Network::Unix,
            }
        }
    }

    pub fn set_name(self, n: &str) -> Self {
        match self.addr {
            NetAddr::Socket(so) => Addr {
                network: self.network,
                addr: NetAddr::NameAndSocket(n.to_string(), so, so.port()),
            },
            NetAddr::Name(_, p) => Addr {
                network: self.network,
                addr: NetAddr::Name(n.to_string(), p),
            },
            NetAddr::NameAndSocket(_, so, p) => Addr {
                network: self.network,
                addr: NetAddr::NameAndSocket(n.to_string(), so, p),
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

    /// 只由 SocketAddr 转, 无视 name. 如果只有 Name, 则返回 None
    pub fn get_socket_addr(&self) -> Option<SocketAddr> {
        if let NetAddr::Socket(s) = &self.addr {
            Some(*s)
        } else if let NetAddr::NameAndSocket(_, so, _) = &self.addr {
            Some(*so)
        } else {
            None
        }
    }

    //todo: DNS 功能
    /// 如果没法从已有的 SocketAddr 转, 则尝试用系统方法解析域名, 并使用第一个值.
    /// 不适用于 UDS
    pub fn get_socket_addr_or_resolve(&self) -> Result<SocketAddr> {
        use std::net::ToSocketAddrs;

        if let NetAddr::Socket(s) = self.addr {
            Ok(s)
        } else if let NetAddr::NameAndSocket(_, so, _) = &self.addr {
            Ok(*so)
        } else if let NetAddr::Name(n, port) = &self.addr {
            let so = (format!("{}:{}", n, port)).to_socket_addrs();
            so?.next()
                .ok_or(anyhow!("resolve to empty socket_addr from {}", self))
        } else {
            Err(anyhow!("not possible convert to socket_addr from {}", self))
        }
    }

    /// only for udp. Unlike try_dial, it will bind to 0.0.0.0:0
    /// to get a random port, then connect to the target addr.
    pub async fn try_dial_udp(&self) -> Result<Stream> {
        match self.network {
            Network::UDP => {
                let so = self.get_socket_addr_or_resolve()?;

                let u = UdpSocket::bind("0.0.0.0:0").await?;
                u.connect(so).await?;
                let mut u = udp::new(u, Some(self.clone()));
                u.default_write_to = Some(self.clone());
                Ok(Stream::AddrConn(u))
            }

            _ => bail!(
                "try_dial_udp failed, not supported network: {}",
                self.network
            ),
        }
    }

    /// dial tcp/udp/unix_domain_socket
    ///
    /// can dial ip if feature "tun" is enabled
    ///
    /// ## udp:
    ///
    /// Addr 的 try_dial 中的 udp 其实是 listen, 它会bind到Addr
    ///
    pub async fn try_dial(&self) -> Result<Stream> {
        match self.network {
            #[cfg(feature = "tun")]
            Network::IP => {
                debug!("Addr dialing {}", self);
                let (tun_name, dial_addr, netmask) = self
                    .to_name_ip_netmask()
                    .context("Addr::try_dial tun, to_name_ip_netmask failed")?;
                let c = tun::dial(tun_name, dial_addr, netmask)
                    .await
                    .context("Addr::try_dial tun, dial failed")?;
                Ok(Stream::Conn(Box::new(c)))
            }
            Network::TCP => {
                let so = self.get_socket_addr_or_resolve()?;

                let c = TcpStream::connect(so).await?;
                Ok(Stream::Conn(Box::new(c)))
            }
            Network::UDP => {
                let so = self.get_socket_addr_or_resolve()?;

                let u = UdpSocket::bind(so).await?;
                let u = udp::new(u, None);
                Ok(Stream::AddrConn(u))
            }
            #[cfg(unix)]
            Network::Unix => {
                let u = UnixStream::connect(self.get_name().unwrap_or_default()).await?;
                Ok(Stream::Conn(Box::new(u)))
            }
            #[cfg(not(feature = "tun"))]
            _ => bail!("try_dial failed, not supported network: {}", self.network),
        }
    }

    ///可为 www.baidu.com:80 或 127.0.0.1:1234 这种形式,
    /// 如果 name和ip都给出了, 首选ip
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
                NetAddr::Socket(so) | NetAddr::NameAndSocket(_, so, _) => so.to_string(),
                NetAddr::Name(n, p) => format!("{}:{}", n, p),
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
    pub fn get_display_addr_str(&self) -> String {
        let mut s = self.get_addr_str();
        match self.network {
            Network::IP => match &self.addr {
                NetAddr::NameAndSocket(n, _, _) => {
                    s.push('#');
                    s.push_str(n);
                }
                _ => {}
            },
            _ => {}
        }
        s
    }

    /// like 10.0.0.1:24. this 24 stores in "port",but means netmask, not port.
    ///
    /// will return (10.0.0.1, 255.255.255.0)
    ///
    /// if it has a name, it will be returned too. might be used as a tun device name
    pub fn to_name_ip_netmask(&self) -> Result<(Option<String>, IpAddr, (u8, u8, u8, u8))> {
        match &self.addr {
            NetAddr::Socket(so) => {
                let nm = prefix_length_to_netmask(so.port() as u8);
                Ok((None, so.ip(), nm))
            }
            NetAddr::NameAndSocket(name, so, port) => {
                let nm = prefix_length_to_netmask(*port as u8);
                Ok((Some(name.clone()), so.ip(), nm))
            }
            _ => bail!("Addr::to_netmask requires addr has ip, you have {:?}", self),
        }
    }
}

/// 以 url 的格式 描述 Addr
impl Display for Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.get_display_addr_str();
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

pub type StreamGenerator = tokio::sync::mpsc::Receiver<MapResult>;

/// default is None
#[derive(Default)]
pub enum Stream {
    ///  rawip / tcp / unix domain socket 等 目标 Addr 唯一的 情况
    Conn(Conn),

    //如果 从 rawip 解析出了 ip 目标, 那么该ip流就是 AddrConn
    /// udp 的情况
    AddrConn(AddrConn),

    /// 比如： tcp listener. Receiver 中的元素为 MapResult, 是为了
    /// 方便传递其它信息, 如peer_addr 由 MapResult.a 标识
    Generator(StreamGenerator),

    #[default]
    None,
}

impl Debug for Stream {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self, f)
    }
}

impl Display for Stream {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self {
            Stream::Conn(c) => write!(f, "{}", c.name()),
            Stream::AddrConn(ac) => write!(f, "{}", ac.name()),
            Stream::Generator(_) => write!(f, "SomeStreamGenerator"),
            Stream::None => write!(f, "NoStream"),
        }
    }
}

impl Stream {
    pub fn c(c: Conn) -> Self {
        Stream::Conn(c)
    }
    pub fn u(u: AddrConn) -> Self {
        Stream::AddrConn(u)
    }
    pub fn g(g: StreamGenerator) -> Self {
        Stream::Generator(g)
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Stream::None)
    }
    pub fn is_some(&self) -> bool {
        !matches!(self, Stream::None)
    }
    pub fn is_generator(&self) -> bool {
        matches!(self, Stream::Generator(_))
    }
    pub fn is_none_or_generator(&self) -> bool {
        matches!(self, Stream::None) || matches!(self, Stream::Generator(_))
    }
    pub async fn try_shutdown(self) -> Result<()> {
        if let Stream::Conn(mut t) = self {
            t.shutdown().await?
        } else if let Stream::AddrConn(mut c) = self {
            c.w.shutdown().await?
        }
        Ok(())
    }

    pub fn try_unwrap_tcp(self) -> Result<Conn> {
        if let Stream::Conn(t) = self {
            return Ok(t);
        }
        Err(anyhow!("not tcp"))
    }

    pub fn try_unwrap_tcp_ref(&self) -> Result<&Conn> {
        if let Stream::Conn(t) = self {
            return Ok(t);
        }
        Err(anyhow!("not tcp"))
    }

    pub fn try_unwrap_udp(self) -> Result<AddrConn> {
        if let Stream::AddrConn(t) = self {
            return Ok(t);
        }
        Err(anyhow!("not udp"))
    }

    pub async fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Conn(conn) => conn.write(buf).await,
            Stream::AddrConn(ac) => {
                let x = ac.default_write_to.as_ref();
                match x {
                    Some(ta) => ac.w.write(buf, ta).await,
                    None => Err(std::io::Error::other(
                        "stream can't write udp directly without a target",
                    )),
                }
            }
            _ => Err(std::io::Error::other("stream is not tcp/udp, can't write")),
        }
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        match self {
            Stream::Conn(conn) => conn.write_all(buf).await,
            Stream::AddrConn(ac) => {
                let x = ac.default_write_to.as_ref();
                match x {
                    Some(ta) => ac.w.write(buf, ta).await.map(|_| ()),
                    None => Err(std::io::Error::other(
                        "stream can't write udp directly without a target",
                    )),
                }
            }
            _ => Err(std::io::Error::other("stream is not tcp/udp, can't write")),
        }
    }
}

use std::sync::atomic::{AtomicU64, Ordering};

use crate::map::MapResult;
use crate::Name;

use self::addr_conn::AddrConn;
use self::addr_conn::AsyncWriteAddrExt;

/// 用于状态监视和流量统计；可以用 Arc<TrafficRecorder> 进行全局的监视和统计。
///
/// ## About Real Data Traffic and Original Traffic
///
/// 注意, 考虑在两个累加结果的Conn之间拷贝, 若用 ruci::net::cp 拷贝并给出 TrafficRecorder,
/// 则它统计出的流量为 未经加密的原始流量, 实际流量一般会比原始流量大。要想用
/// ruci::net::cp 统计真实流量, 只能有一种情况, 那就是 tcp到tcp的直接拷贝,
/// 不使用累加器。
///
/// 一种统计正确流量的办法是, 将 Tcp连接包装一层专门记录流量的层, 见 counter 模块
///
#[derive(Debug, Default)]
pub struct TrafficRecorder {
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

#[cfg(unix)]
impl crate::Name for UnixStream {
    fn name(&self) -> &str {
        "unixstream"
    }
}

/// an important type in ruci
pub type Conn = Box<dyn ConnTrait>;

/// may log debug or do other side-effect stuff with id.
pub async fn cp<C1: ConnTrait, C2: ConnTrait>(
    c1: C1,
    c2: C2,
    cid: &CID,
    opt: Option<Arc<TrafficRecorder>>,
) -> Result<u64, Error> {
    if log_enabled!(log::Level::Debug) {
        debug!("cp start, {} c1: {}, c2: {}", cid, c1.name(), c2.name());
    }

    let (mut c1_read, mut c1_write) = tokio::io::split(c1);
    let (mut c2_read, mut c2_write) = tokio::io::split(c2);

    let (c1_to_c2, c2_to_c1) = (
        tokio::io::copy(&mut c1_read, &mut c2_write).fuse(),
        tokio::io::copy(&mut c2_read, &mut c1_write).fuse(),
    );

    pin_mut!(c1_to_c2, c2_to_c1);

    // 一个方向停止后, 关闭连接, 如果 opt 不为空, 则等待另一个方向关闭, 以获取另一方向的流量信息。

    futures::select! {
        r1 = c1_to_c2 => {
            if let Some(ref tr) = opt {
                if let Ok(n) = r1 {
                    let tt = tr.ub.fetch_add(n, Ordering::Relaxed);

                    if log_enabled!(log::Level::Debug) {
                        debug!("cp, {}, u, ub, {}, {}",cid,n,tt+n);
                    }
                }

                // can't borrow mut more than once. We just hope tokio will shutdown tcp
                // when it's dropped.
                // during the tests we can prove it's dropped.

                let r2 = c2_to_c1.await;
                if let Some(ref tr) = opt {
                    if let Ok(n) = r2 {
                        let tt = tr.db.fetch_add(n, Ordering::Relaxed);

                        if log_enabled!(log::Level::Debug) {
                            debug!("cp, {}, u, db, {}, {}",cid, n,tt+n);
                        }
                    }
                }
            }


            if log_enabled!(log::Level::Debug) {
                debug!("cp end u, {} ",cid);
            }

            r1
        },
        r2 = c2_to_c1 => {
            if let Some(ref tr) = opt {
                if let Ok(n) = r2 {
                    let tt = tr.db.fetch_add(n, Ordering::Relaxed);

                    if log_enabled!(log::Level::Debug) {
                        debug!("cp, {}, d, db, {}, {}",cid, n,tt+n);
                    }
                }

                let r1 = c1_to_c2.await;
                if let Some(ref tr) = opt {
                    if let Ok(n) = r1 {
                        let tt = tr.ub.fetch_add(n, Ordering::Relaxed);

                        if log_enabled!(log::Level::Debug) {
                            debug!("cp, {}, d, ub, {}, {}",cid,n,tt+n);
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
