use std::collections::HashMap;
use std::net::IpAddr;
use std::net::SocketAddr;

use futures::executor::block_on;
use hickory_resolver::config::*;
use hickory_resolver::Resolver;
use hickory_resolver::TokioAsyncResolver;

pub type TheLookupIpStrategy = LookupIpStrategy;
pub type TheProtocol = Protocol;

pub fn get_config(dns_server_list: Vec<(SocketAddr, Protocol)>) -> ResolverConfig {
    let mut cf = if dns_server_list.is_empty() {
        ResolverConfig::default()
    } else {
        ResolverConfig::new()
    };

    for (sa, p) in dns_server_list {
        cf.add_name_server(NameServerConfig::new(sa, p))
    }
    cf
}

pub fn create_resolver(
    dns_server_list: Vec<(SocketAddr, Protocol)>,
    ip_strategy: Option<LookupIpStrategy>,
) -> Resolver {
    let cf = get_config(dns_server_list);

    let mut ro = ResolverOpts::default();
    if let Some(is) = ip_strategy {
        ro.ip_strategy = is; // 不设时，默认是先4后6
    }

    Resolver::new(cf, ro).unwrap()
}

pub fn create_async_resolver(
    dns_server_list: Vec<(SocketAddr, Protocol)>,
    ip_strategy: Option<LookupIpStrategy>,
) -> TokioAsyncResolver {
    let cf = get_config(dns_server_list);

    let mut ro = ResolverOpts::default();
    if let Some(is) = ip_strategy {
        ro.ip_strategy = is; // 不设时，默认是先4后6
    }

    // https://docs.rs/hickory-resolver/0.24.1/hickory_resolver/
    block_on(async { TokioAsyncResolver::tokio(cf, ro) })
}

// pub struct NamePortAndClient<'a>(pub &'a str, pub u16, pub &'a AsyncClient);

// // https://internals.rust-lang.org/t/custom-global-dns-resolver/18667/5
// impl std::net::ToSocketAddrs for NamePortAndClient<'_> {
//     type Iter = std::option::IntoIter<SocketAddr>;

//     fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
//         let x = block_on(self.2.lookup(self.0)).map(|ip| SocketAddr::new(ip, self.1));
//         let x = x.into_iter();
//         Ok(x)
//     }
// }

#[derive(Debug, Clone)]
pub struct AsyncClient {
    pub r: TokioAsyncResolver,

    pub static_pairs: HashMap<String, IpAddr>,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClientConfig {
    pub dns_server_list: Vec<(SocketAddr, Protocol)>,
    pub ip_strategy: Option<LookupIpStrategy>,
    pub static_pairs: HashMap<String, IpAddr>,
}

impl AsyncClient {
    pub fn new(c: ClientConfig) -> Self {
        let r = create_async_resolver(c.dns_server_list, c.ip_strategy);
        Self {
            r,
            static_pairs: c.static_pairs,
        }
    }

    pub fn check_cache(&self, name: &str) -> Option<IpAddr> {
        self.static_pairs.get(name).copied()
    }

    /// better passin fully-qualified-domain-name, FQDN, which ends in a final `.`.
    pub async fn lookup(&self, name: &str) -> Option<IpAddr> {
        if let Some(ip) = self.check_cache(name) {
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    name = %name,
                    ip = %ip,
                    "resolved by static",
                );
            }

            return Some(ip);
        }

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                name = %name,
                "resolving",
            );
        }

        let mut n = name;
        let nstring: String;
        if !n.ends_with(".") {
            nstring = name.to_owned() + ".";
            n = &nstring;
        }

        let resolver = &self.r;

        let response = resolver.lookup_ip(n).await.unwrap();

        let address: Vec<_> = response.iter().collect();
        if address.is_empty() {
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    name = %name,
                    "resolved to empty",
                );
            }
            None
        } else {
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    name = %name,
                    address = ?address,
                    "resolved",
                );
            }

            address.into_iter().next()
        }
    }

    pub async fn lookup_v4(&self, name: &str) -> Option<IpAddr> {
        if let Some(ip) = self.check_cache(name) {
            return Some(ip);
        }

        let mut n = name;
        let nstring: String;
        if !n.ends_with(".") {
            nstring = name.to_owned() + ".";
            n = &nstring;
        }

        let resolver = &self.r;

        let response = resolver.ipv4_lookup(n).await.unwrap();

        let address = response.iter().next();

        address.map(|ip| IpAddr::V4(ip.0))
    }

    pub async fn lookup_v6(&self, name: &str) -> Option<IpAddr> {
        if let Some(ip) = self.check_cache(name) {
            return Some(ip);
        }

        let mut n = name;
        let nstring: String;
        if !n.ends_with(".") {
            nstring = name.to_owned() + ".";
            n = &nstring;
        }

        let resolver = &self.r;

        let response = resolver.ipv6_lookup(n).await.unwrap();

        let address = response.iter().next();

        address.map(|ip| IpAddr::V6(ip.0))
    }
}

// #[tokio::test]
#[allow(dead_code)]
async fn test() {
    let sa = std::net::SocketAddr::V4("0.0.0.0:20800".parse().unwrap());

    let cc = ClientConfig {
        dns_server_list: vec![(sa, TheProtocol::Udp)],
        ip_strategy: Some(TheLookupIpStrategy::Ipv4Only),
        static_pairs: HashMap::new(),
    };

    let ac = AsyncClient::new(cc);

    println!("{:?}", ac.lookup("www.baidu.com").await);
}
