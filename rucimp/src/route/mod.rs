/*!
 *

route 模块中的定义的是 比 ruci::route中的 InboundInfoOutSelector 更实用的 OutSelector

加了很多范围匹配

有 WhiteList 和 BlackList 两种模式


与 verysimple 一样, 我们直接使用 maxmind 的 数据 作为ip国别判断的数据库

https://github.com/Loyalsoldier/geoip

https://github.com/oschwald/maxminddb-rust



*/

#[cfg(feature = "geoip")]
mod geoip;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use ipnet::*;
use iprange::IpRange;
use regex::RegexSet;
use ruci::{
    map::{AnyData, MIterBox},
    net::{self, *},
    relay::route::{self, *},
    user::*,
};

#[derive(Debug)]
pub struct RuleSetOutSelector {
    pub outbounds_rules_vec: Vec<RuleSet>, // rule -> out_tag
    pub outbounds_map: Arc<HashMap<String, MIterBox>>, //out_tag -> outbound
    pub default: MIterBox,
}

#[async_trait]
impl route::OutSelector for RuleSetOutSelector {
    async fn select(
        &self,
        addr: &net::Addr,
        in_chain_tag: &str,
        params: &Vec<Option<AnyData>>,
    ) -> MIterBox {
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

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Mode {
    BlackList,
    WhiteList,
}

/// ta 前缀 意思是 target_addr,
#[derive(Clone, Debug)]
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

        let is_in_ta_ip_countries = self.is_in_ta_ip_countries(true, &r.target_addr);
        if !is_in_ta_ip_countries {
            return false;
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

        let is_in_ta_ip_countries = self.is_in_ta_ip_countries(true, &r.target_addr);
        if is_in_ta_ip_countries {
            return true;
        }
        false
    }

    /// 如果在集合中, 或 allow_empty 且 集合为 空, 返回 true
    pub fn is_in_in_tags(&self, allow_empty: bool, in_chain_tag: &str) -> bool {
        match &self.in_tags {
            Some(ts) => ts.get(in_chain_tag).is_some(),
            None => allow_empty,
        }
    }

    pub fn is_in_userset(&self, allow_empty: bool, users: &UserVec) -> bool {
        match &self.userset {
            Some(us) => us.get(users).is_some(),
            None => allow_empty,
        }
    }

    pub fn is_in_ips(&self, allow_empty: bool, addr: &net::Addr) -> bool {
        match addr.addr {
            NetAddr::Socket(so) | NetAddr::NameAndSocket(_, so, _) => match so {
                std::net::SocketAddr::V4(so) => match &self.ta_ipv4 {
                    Some(i4) => i4.contains(so.ip()),
                    None => allow_empty,
                },
                std::net::SocketAddr::V6(so) => match &self.ta_ipv6 {
                    Some(i6) => i6.contains(so.ip()),
                    None => allow_empty,
                },
            },
            NetAddr::Name(_, _) => false,
        }
    }

    pub fn is_in_networks(&self, allow_empty: bool, addr: &net::Addr) -> bool {
        match &self.ta_networks {
            Some(nw) => nw.get(&addr.network).is_some(),
            None => allow_empty,
        }
    }

    pub fn is_in_domain(&self, allow_empty: bool, addr: &net::Addr) -> bool {
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
                _ => allow_empty,
            },
            None => allow_empty,
        }
    }

    pub fn is_in_ta_ip_countries(&self, allow_empty: bool, addr: &net::Addr) -> bool {
        #[cfg(feature = "geoip")]
        let is_in = {
            match &self.ta_ip_countries {
                Some(cs) => match addr.addr {
                    NetAddr::Socket(so) | NetAddr::NameAndSocket(_, so, _) => {
                        let ip = so.ip();
                        let country = geoip::get_ip_iso(ip);
                        cs.contains(&country)
                    }
                    _ => allow_empty,
                },
                None => allow_empty,
            }
        };
        #[cfg(not(feature = "geoip"))]
        let is_in = { allow_empty };

        is_in
    }
}
// todo : add test
