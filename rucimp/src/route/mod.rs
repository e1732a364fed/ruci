/*!
 *

route 模块中的定义的是 比 ruci::route中的 InboundInfoOutSelector 更实用的 OutSelector

加了很多范围匹配

有 WhiteList 和 BlackList 两种模式



*/

#[cfg(feature = "geoip")]
pub mod maxmind;

pub mod config;

use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    sync::Arc,
};

use async_trait::async_trait;
use ipnet::*;
use iprange::IpRange;
use regex::RegexSet;
use ruci::{
    map::{acc::DMIterBox, Data},
    net::{self, *},
    relay::route::{self, *},
    user::*,
};

#[derive(Debug)]
pub struct RuleSetOutSelector {
    pub outbounds_rules_vec: Vec<RuleSet>, // rule -> out_tag
    pub outbounds_map: Arc<HashMap<String, DMIterBox>>, //out_tag -> outbound
    pub default: DMIterBox,
}

#[async_trait]
impl route::OutSelector for RuleSetOutSelector {
    async fn select(&self, addr: &net::Addr, in_chain_tag: &str, params: &[Data]) -> DMIterBox {
        let users = get_user_from_anydata_vec(params).await;
        let r = InboundInfo {
            in_tag: in_chain_tag.to_string(),
            target_addr: addr.clone(),
            users,
        };
        let mut out_tag: Option<String> = None;
        for rs in self.outbounds_rules_vec.iter() {
            if rs.matches(&r) {
                out_tag = Some(rs.out_tag.clone());
                break;
            }
        }

        match out_tag {
            Some(out_k) => {
                let y = self.outbounds_map.get(&out_k);
                match y {
                    Some(out) => out.clone(),
                    None => self.default.clone(),
                }
            }
            None => self.default.clone(),
        }
        //todo: add test
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub enum Mode {
    BlackList,

    #[default]
    WhiteList,
}

/// ta 前缀 意思是 target_addr,
#[derive(Clone, Default)]
pub struct RuleSet {
    pub out_tag: String,

    pub mode: Mode,
    pub in_tags: Option<HashSet<String>>,

    /// 一个条代理链握手成功后可能出现多个层的user, 即每条链都有一个 UserVec数据
    pub userset: Option<HashSet<UserVec>>,

    pub ta_ip_countries: Option<HashSet<String>>,

    pub ta_networks: Option<HashSet<Network>>,

    pub ta_ipv4: Option<IpRange<Ipv4Net>>,
    pub ta_ipv6: Option<IpRange<Ipv6Net>>,

    pub ta_domain_matcher: Option<DomainMatcher>,
    /// for geoip, checkiing ip_countries
    #[cfg(feature = "geoip")]
    pub mmdb_reader: Option<Arc<maxminddb::Reader<Vec<u8>>>>,
}
//todo: add peer_addr related filter

impl Debug for RuleSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuleSet")
            .field("out_tag", &self.out_tag)
            .field("mode", &self.mode)
            .field("in_tags", &self.in_tags)
            .field("userset", &self.userset)
            .field("ta_ip_countries", &self.ta_ip_countries)
            .field("ta_networks", &self.ta_networks)
            .field("ta_ipv4", &self.ta_ipv4)
            .field("ta_ipv6", &self.ta_ipv6)
            .field("ta_domain_matcher", &self.ta_domain_matcher)
            //.field("mmdb_reader", &self.mmdb_reader) //print this would spam the console
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct DomainMatcher {
    pub domain_regex: Option<RegexSet>,
    pub domain_set: Option<HashSet<String>>,
}

impl RuleSet {
    pub fn matches(&self, r: &InboundInfo) -> bool {
        match self.mode {
            Mode::BlackList => self.matches_blacklist(r),
            Mode::WhiteList => self.matches_whitelist(r),
        }
    }

    /// 只有所有的匹配项全通过, 才返回 true
    pub fn matches_whitelist(&self, r: &InboundInfo) -> bool {
        let is_in_in_tags = self.is_in_in_tags(true, &r.in_tag);
        if !is_in_in_tags {
            return false;
        }

        let is_in_userset = match &r.users {
            Some(us) => self.is_in_userset(true, us),
            None => self.userset.is_none(),
        };
        if !is_in_userset {
            return false;
        }

        let is_in_networks = self.is_in_networks(true, &r.target_addr);
        if !is_in_networks {
            return false;
        }

        let ip_is_in = self.is_in_ips(true, &r.target_addr);
        if !ip_is_in {
            return false;
        }

        #[cfg(feature = "geoip")]
        {
            let is_in_ta_ip_countries = self.is_in_ta_ip_countries(true, &r.target_addr);
            if !is_in_ta_ip_countries {
                return false;
            }
        }

        true
    }

    /// 有一个匹配项通过, 就返回 true
    pub fn matches_blacklist(&self, r: &InboundInfo) -> bool {
        let is_in_in_tags = self.is_in_in_tags(false, &r.in_tag);
        if is_in_in_tags {
            return true;
        }

        let is_in_userset = match &r.users {
            Some(us) => self.is_in_userset(false, us),
            None => self.userset.is_some(),
        };
        if is_in_userset {
            return true;
        }

        let is_in_networks = self.is_in_networks(true, &r.target_addr);
        if is_in_networks {
            return true;
        }

        let ip_is_in = self.is_in_ips(false, &r.target_addr);
        if ip_is_in {
            return true;
        }

        #[cfg(feature = "geoip")]
        {
            let is_in_ta_ip_countries = self.is_in_ta_ip_countries(true, &r.target_addr);
            if is_in_ta_ip_countries {
                return true;
            }
        }
        false
    }

    /// 如果在集合中, 或 true_if_empty 且 集合为 空, 返回 true
    pub fn is_in_in_tags(&self, true_if_empty: bool, in_chain_tag: &str) -> bool {
        match &self.in_tags {
            Some(ts) => ts.get(in_chain_tag).is_some(),
            None => true_if_empty,
        }
    }

    pub fn is_in_userset(&self, true_if_empty: bool, users: &UserVec) -> bool {
        match &self.userset {
            Some(us) => us.get(users).is_some(),
            None => true_if_empty,
        }
    }

    pub fn is_in_ips(&self, true_if_empty: bool, addr: &net::Addr) -> bool {
        match addr.addr {
            NetAddr::Socket(so) | NetAddr::NameAndSocket(_, so, _) => match so {
                std::net::SocketAddr::V4(so) => match &self.ta_ipv4 {
                    Some(i4) => i4.contains(so.ip()),
                    None => true_if_empty,
                },
                std::net::SocketAddr::V6(so) => match &self.ta_ipv6 {
                    Some(i6) => i6.contains(so.ip()),
                    None => true_if_empty,
                },
            },
            NetAddr::Name(_, _) => false,
        }
    }

    pub fn is_in_networks(&self, true_if_empty: bool, addr: &net::Addr) -> bool {
        match &self.ta_networks {
            Some(nw) => nw.get(&addr.network).is_some(),
            None => true_if_empty,
        }
    }

    pub fn is_in_domain(&self, true_if_empty: bool, addr: &net::Addr) -> bool {
        match &self.ta_domain_matcher {
            Some(dm) => match &addr.addr {
                NetAddr::Name(domain, _) => {
                    if let Some(dmr) = &dm.domain_regex {
                        if dmr.is_match(&domain) {
                            return true;
                        }
                    }
                    if let Some(dms) = &dm.domain_set {
                        if dms.contains(domain) {
                            return true;
                        }
                    }
                    false
                }
                _ => true_if_empty,
            },
            None => true_if_empty,
        }
    }

    #[cfg(feature = "geoip")]
    pub fn is_in_ta_ip_countries(&self, true_if_empty: bool, addr: &net::Addr) -> bool {
        match &self.mmdb_reader {
            None => true_if_empty,

            Some(mr) => match &self.ta_ip_countries {
                None => true_if_empty,

                Some(cs) => match addr.addr {
                    NetAddr::Socket(so) | NetAddr::NameAndSocket(_, so, _) => {
                        let ip = so.ip();
                        let str = &maxmind::get_ip_iso_by_reader(ip, mr);
                        let country = maxmind::filter_iso_string_to_iso3166(str);
                        cs.contains(country)
                    }
                    _ => true_if_empty,
                },
            },
        }
    }
}

#[cfg(test)]
mod test {

    use std::net::Ipv4Addr;

    use crate::COMMON_DIRS;

    use super::*;

    #[test]
    fn tst_iprange() -> anyhow::Result<()> {
        let ip_range: IpRange<Ipv4Net> = ["172.16.0.0/16", "192.168.1.0/24"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();

        for network in &ip_range {
            println!("{:?}", network);
        }

        let ip: Ipv4Addr = "192.168.1.1".parse()?;
        let r = ip_range.contains(&ip);
        assert!(r);

        let ip: Ipv4Addr = "192.168.1.100".parse()?;
        let r = ip_range.contains(&ip);
        assert!(r);

        let ip: Ipv4Addr = "192.168.2.1".parse()?;
        let r = ip_range.contains(&ip);
        assert!(!r);
        Ok(())
    }

    #[test]
    fn tst_net() -> anyhow::Result<()> {
        let mut rs = RuleSet::default();

        let mut nets = HashSet::new();
        nets.insert(Network::UDP);

        rs.ta_networks = Some(nets);
        let a = Addr::from_network_addr_str("tcp://104.193.88.123:80")?;

        let r = rs.is_in_networks(false, &a);
        assert!(!r);

        let a = Addr::from_network_addr_str("udp://1.1.1.1:80")?;

        let r = rs.is_in_networks(false, &a);
        assert!(r);

        Ok(())
    }

    #[test]
    #[cfg(feature = "geoip")]
    fn test_country() -> anyhow::Result<()> {
        let mut rs = RuleSet::default();
        let mr = maxmind::open_mmdb("Country.mmdb", &COMMON_DIRS)?;
        rs.mmdb_reader = Some(Arc::new(mr));

        let mut ipcountries = HashSet::new();
        ipcountries.insert("CN".to_string());
        ipcountries.insert("US".to_string());

        rs.ta_ip_countries = Some(ipcountries);

        //www.baidu.com's IP
        let a = Addr::from_network_addr_str("tcp://104.193.88.123:80")?;

        let r = rs.is_in_ta_ip_countries(false, &a);
        assert!(r);

        // www.google.com's IP
        let a = Addr::from_network_addr_str("tcp://142.251.32.36:80")?;

        let r = rs.is_in_ta_ip_countries(false, &a);
        assert!(r);

        Ok(())
    }
}
