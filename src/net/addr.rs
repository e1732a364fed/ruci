/*!
Provides facilities to generate, represent and decompose network addresses,
by important parts like [`Network`], [`NetAddr`], [`IPName`], and [`Addr`].
 */
use super::*;

#[allow(unused)]
use anyhow::Context;
use anyhow::Result;
use tokio::net::TcpSocket;

pub fn ip_addr_to_u8_vec(ip_addr: IpAddr) -> Vec<u8> {
    match ip_addr {
        IpAddr::V4(v4) => v4.octets().to_vec(),
        IpAddr::V6(v6) => v6.octets().to_vec(),
    }
}

pub fn gen_random_ipv6() -> IpAddr {
    let mut rng = rand::rng();
    let mut octets = [0; 16];
    rng.fill(&mut octets);
    IpAddr::V6(Ipv6Addr::from(octets))
}

/// 1024..=65535
pub fn gen_random_port() -> u16 {
    let mut rng = rand::rng();
    rng.random_range(1024..=65535)
}

///10240..=65535
pub fn gen_random_higher_port() -> u16 {
    let mut rng = rand::rng();
    rng.random_range(10240..=65535)
}

/// This function might work better than using a.eq(b).
///
/// If a and b are all unspecified then `true` will also be returned)
///
pub fn eq_socket_addr(a: &SocketAddr, b: &SocketAddr) -> bool {
    if a.eq(b) {
        true
    } else if a.port() != b.port() {
        false
    } else {
        a.ip().is_unspecified() || b.ip().is_unspecified()
    }
}

/// default: ipv4 0.0.0.0:0
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
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
            _ => bail!("not supported network string: {}", s),
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
/// 具体是哪一种由 network 决定. 若 network 不为 unix,
/// addr 可以为 Socket 或 Name (表示 domain name),
/// 否则 addr 只能为 Name (表示 file name)
///
/// port = 0 表示不用端口, 或表示让系统在拨号时使用系统分配的端口
///
/// Addr实现 Eq和 Hash, 以支持作为Key存入 HashMap 等集合中.
///
/// default is  tcp://0.0.0.0:0
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
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
    /// `tcp://[::1]:80#www.b.com`
    ///
    /// if no "#", it will fallback to from_network_addr_str
    ///
    pub fn from_name_network_addr_url(s: &str) -> Result<Self> {
        let ns: Vec<_> = s.splitn(2, '#').collect();
        match ns.len() {
            1 => Addr::from_network_addr_url(s),
            2 => {
                let a = Addr::from_network_addr_url(ns[0])?;
                Ok(a.set_name(ns[1]))
            }
            _ => bail!("Addr::from_name_network_addr_str, split # got len!=2 && len!=1",),
        }
    }

    /// tcp://127.0.0.1:80 or tcp://www.b.com:80.  or `tcp://[::1]:80`
    ///
    /// if :// is not present, use tcp as network, like 1.1.1.1:1 will act like
    /// tcp://1.1.1.1:1
    pub fn from_network_addr_url(s: &str) -> Result<Self> {
        let ns: Vec<_> = s.splitn(2, "://").collect();
        match ns.len() {
            1 => Addr::from_addr_str("tcp", s),
            2 => Addr::from_addr_str(ns[0], ns[1]),
            _ => bail!("Addr::from_network_addr_str, split :// got len!=2 && len!=1",),
        }
    }

    /// "tcp",127.0.0.1:80 or "tcp",www.b.com:80. or "tcp", `[::1]:80`
    ///
    ///  if unix, then like path/to/file, without the port and colon.
    ///
    /// network must be a valid network str
    pub fn from_addr_str(network: &str, s: &str) -> Result<Self> {
        let ns: Vec<_> = if s.starts_with('[') && s.contains("]:") {
            crate::utils::rm_first(s).split("]:").collect()
        } else {
            s.split(':').collect()
        };
        let port = if ns.len() != 2 {
            0
        } else {
            ns[1].parse::<u16>().map_err(|e| anyhow!("{}", e))?
        };

        match ns[0].parse::<IpAddr>() {
            Ok(ip) => Addr::from(network, None, Some(ip), port),
            Err(_) => Addr::from(network, Some(ns[0].to_string()), None, port),
        }
    }

    /// like 127.0.0.1:80  or `[::1]:80`
    pub fn from_ip_addr_str(network: &'static str, s: &str) -> Result<Self> {
        let ns: Vec<_> = if s.starts_with('[') && s.contains("]:") {
            crate::utils::rm_first(s).split("]:").collect()
        } else {
            s.split(':').collect()
        };

        if ns.len() != 2 {
            bail!(
                "Addr::from_ip_addr_str, split colon got len!=2, found len={}",
                ns.len()
            );
        }
        Addr::from_strs(
            network,
            "",
            ns[0],
            ns[1].parse::<u16>().map_err(|e| anyhow!("{}", e))?,
        )
    }

    /// Constructs an `Addr` from an `IPName` and port, setting the network to TCP.
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
    /// Constructs an `Addr` from a Unix socket address.
    pub fn from_unix(unix_soa: tokio::net::unix::SocketAddr) -> Self {
        if unix_soa.is_unnamed() {
            Addr {
                addr: NetAddr::Name("".to_string(), 0),
                network: Network::Unix,
            }
        } else {
            let p = unix_soa
                .as_pathname()
                .expect("Unix socket address should have a pathname")
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

    /// used for get a domain
    pub fn get_name_or_ip_string(&self) -> Option<String> {
        let on = self.get_name();
        if on.is_none() {
            Some(match &self.addr {
                NetAddr::Socket(so) => so.ip().to_string(),
                NetAddr::Name(n, _) => n.clone(),
                NetAddr::NameAndSocket(_, ip, _) => ip.to_string(),
            })
        } else {
            on
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

    /// 从 self 中提取 SocketAddr.
    ///
    /// 如果没法从已有的 SocketAddr 转, 若提供了 dns::AsyncClient 则用其解析；
    /// 若未提供，则尝试用系统方法解析域名, 并使用第一个值.
    /// 不适用于 UDS
    pub async fn get_socket_addr_or_resolve(
        &self,
        oc: Option<&dns::AsyncClient>,
    ) -> Result<SocketAddr> {
        use std::net::ToSocketAddrs;

        if let NetAddr::Socket(s) = self.addr {
            Ok(s)
        } else if let NetAddr::NameAndSocket(_, so, _) = &self.addr {
            Ok(*so)
        } else if let NetAddr::Name(n, port) = &self.addr {
            let so = if let Some(ac) = oc {
                return ac
                    .lookup(n)
                    .await
                    .map(|ipa| SocketAddr::new(ipa, *port))
                    .ok_or(anyhow!("resolve to empty socket_addr from {}", self));
            } else {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        name = %n,
                        "resolve by system",
                    );
                }

                (format!("{}:{}", n, port)).to_socket_addrs()?.next()
            };

            so.ok_or(anyhow!("resolve to empty socket_addr from {}", self))
        } else {
            bail!("not possible convert to socket_addr from {}", self)
        }
    }

    /// only for udp. Unlike try_dial, it will bind to 0.0.0.0:0
    /// to get a random port, then connect to the target addr(self).
    pub async fn try_dial_udp(&self, oc: Option<Arc<dns::AsyncClient>>) -> Result<Stream> {
        match self.network {
            Network::UDP => {
                let so = self.get_socket_addr_or_resolve(oc.as_deref()).await?;

                let u = UdpSocket::bind("0.0.0.0:0").await?;
                u.connect(so).await?;
                let mut u = udp::new(u, Some(self.clone()), false, oc);
                u.default_write_to = Some(self.clone());
                Ok(Stream::AddrConn(u))
            }

            _ => bail!(
                "try_dial_udp failed, not supported network: {}",
                self.network
            ),
        }
    }

    /// dial tcp/udp/unix_domain_socket and `ip` if feature "tun" is enabled
    ///
    /// ## udp
    ///
    /// 本函数对 udp 的作用 其实是 listen, 它会bind到Addr
    ///
    pub async fn try_dial(&self, oc: Option<Arc<dns::AsyncClient>>) -> Result<Stream> {
        match self.network {
            #[cfg(feature = "tun")]
            Network::IP => {
                debug!("Addr dialing IP {}", self);
                let (tun_name, dial_addr, netmask) = self
                    .to_name_ip_netmask()
                    .context("Addr::try_dial tun, to_name_ip_netmask failed")?;
                let c = tun::create_bind(tun_name, dial_addr, netmask)
                    .await
                    .context("Addr::try_dial tun, dial failed")?;
                Ok(Stream::Conn(Box::new(c)))
            }
            Network::TCP => {
                let so = self.get_socket_addr_or_resolve(oc.as_deref()).await?;

                let c = TcpStream::connect(so).await?;
                Ok(Stream::Conn(Box::new(c)))
            }
            Network::UDP => {
                let so = self.get_socket_addr_or_resolve(oc.as_deref()).await?;

                let u = UdpSocket::bind(so).await?;
                let u = udp::new(u, None, false, oc);
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

    /// bind_dial 避免了 try_dial 对 bind 和 connect 地址的混淆
    ///
    /// ## bind_a
    ///
    /// 对于 ip, bind_a 须提供, 否则将报错
    ///
    /// 对于 tcp/udp, 如果 bind_a 不提供, 将采用 随机端口
    ///
    /// 对于 uds, bind_a 无意义
    ///
    /// ## dial_a
    ///
    /// 对于 ip, dial_a 无意义
    ///
    /// 对于 tcp/uds, dial_a 须提供，否则将报错
    ///
    /// ## udp_fix_target_listen
    ///
    /// 为 true 时, 会对udp 做针对 fixed_target_addr 的 特殊处理,
    /// 对 非 udp 情况 无意义
    ///
    pub async fn bind_dial(
        bind_a: Option<&Self>,
        dial_a: Option<&Self>,
        udp_fix_target_listen: Option<bool>,
        oc: Option<Arc<dns::AsyncClient>>,
    ) -> Result<Stream> {
        if bind_a.is_none() && dial_a.is_none() {
            bail!("bind_dial: bind_a and dial_a are both none");
        }
        let network = bind_a
            .as_ref()
            .map(|a| a.network.clone())
            .or_else(|| dial_a.as_ref().map(|a| a.network.clone()))
            .unwrap();

        match network {
            #[cfg(feature = "tun")]
            Network::IP => {
                let ip = match &bind_a {
                    Some(a) => a,
                    None => bail!("bind_a is required for binding ip"),
                };
                debug!("Addr binding IP {}", ip);
                let (tun_name, dial_addr, netmask) = ip
                    .to_name_ip_netmask()
                    .context("Addr::bind_dial tun, to_name_ip_netmask failed")?;
                // let rw = tun::create_bind_rw(tun_name, dial_addr, netmask)
                //     .await
                //     .context("bind_dial failed for tun")?;
                // Ok(Stream::RW(rw))

                let c = tun::create_bind(tun_name, dial_addr, netmask)
                    .await
                    .context("bind_dial failed for tun")?;
                Ok(Stream::Conn(c))
            }
            Network::TCP => {
                let dial_a = match &dial_a {
                    Some(a) => a,
                    None => bail!("bind_dial: tcp must provide dial_a "),
                };
                let c = match bind_a {
                    Some(bind_a) => {
                        let bind_so = bind_a.get_socket_addr_or_resolve(oc.as_deref()).await?;
                        let socket = if bind_so.is_ipv4() {
                            TcpSocket::new_v4()?
                        } else {
                            TcpSocket::new_v6()?
                        };
                        socket.bind(bind_so)?;

                        let dial_so = dial_a.get_socket_addr_or_resolve(oc.as_deref()).await?;

                        socket.connect(dial_so).await?
                    }
                    None => {
                        let dial_so = dial_a.get_socket_addr_or_resolve(oc.as_deref()).await?;

                        TcpStream::connect(dial_so).await?
                    }
                };
                Ok(Stream::Conn(Box::new(c)))
            }
            Network::UDP => {
                let bind_so = match bind_a {
                    Some(a) => a.get_socket_addr_or_resolve(oc.as_deref()).await?,
                    None => {
                        Self::default()
                            .get_socket_addr_or_resolve(oc.as_deref())
                            .await?
                    }
                };

                let u = UdpSocket::bind(bind_so).await?;
                let u = match dial_a {
                    None => udp::new(u, None, udp_fix_target_listen.unwrap_or_default(), oc),

                    Some(dial_a) => {
                        let dial_so = dial_a.get_socket_addr_or_resolve(oc.as_deref()).await?;

                        u.connect(dial_so).await?;

                        udp::new(
                            u,
                            Some(dial_a.clone()),
                            udp_fix_target_listen.unwrap_or_default(),
                            oc,
                        )
                    }
                };
                Ok(Stream::AddrConn(u))
            }
            #[cfg(unix)]
            Network::Unix => {
                let dial_a = match dial_a {
                    Some(a) => a,
                    None => bail!("bind_dial: uds must provide dial_a "),
                };

                let u = UnixStream::connect(dial_a.get_name().unwrap_or_default()).await?;
                Ok(Stream::Conn(Box::new(u)))
            }
            #[cfg(not(feature = "tun"))]
            _ => bail!("bind_dial failed, not supported network: {:?}", network),
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
        if self.network == Network::IP {
            if let NetAddr::NameAndSocket(n, _, _) = &self.addr {
                s.push('#');
                s.push_str(n);
            }
        }
        s
    }

    /// like 10.0.0.1:24. this 24 stores in "port",but means netmask, not port.
    ///
    /// will return (10.0.0.1, 255.255.255.0)
    ///
    /// if it has a name, it will be returned too. might be used as a tun device name
    pub fn to_name_ip_netmask(&self) -> Result<NameIpNetMask> {
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

pub type NameIpNetMask = (Option<String>, IpAddr, (u8, u8, u8, u8));

/// 以 url 的格式 描述 Addr
impl Display for Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.get_display_addr_str();
        write!(f, "{}://{}", self.network.to_static_str(), s)
    }
}
