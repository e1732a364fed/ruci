/*!
 *

route 模块中的定义的是 比 ruci::route中的 InboundInfoOutSelector 更实用的 OutSelector

加了很多范围匹配

有 WhiteList 和 BlackList 两种模式


与 verysimple 一样, 我们直接使用 maxmind 的 数据 作为ip国别判断的数据库

https://github.com/Loyalsoldier/geoip

https://github.com/oschwald/maxminddb-rust



*/
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use ipnet::*;
use iprange::IpRange;
use regex::RegexSet;
use ruci::{
    map::MIterBox,
    net::{self, *},
    relay::route::InboundInfo,
    user::*,
};

#[derive(Debug)]
pub struct RuleSetOutSelector {
    pub outbounds_rules_vec: Vec<RuleSet>, // rule -> out_tag
    pub outbounds_map: Arc<HashMap<String, MIterBox>>, //out_tag -> outbound
    pub default: MIterBox,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Mode {
    BlackList,
    WhiteList,
}

#[derive(Clone, Debug)]
pub struct RuleSet {
    pub out_tag: String,

    pub mode: Mode,
    pub in_tags: Vec<String>,

    /// 一个条代理链握手成功后可能出现多个层的user, 即每条链都有一个 UserVec数据
    pub users_list: Vec<UserVec>,

    pub countries: Vec<String>,

    pub networks: Vec<Network>,

    pub ipv4: IpRange<Ipv4Net>,
    pub ipv6: IpRange<Ipv6Net>,
    pub rule_regex: RegexSet,
    pub rule_set: HashSet<String>,
}

impl RuleSet {
    pub fn matches(&self, r: &InboundInfo) -> bool {
        match self.mode {
            Mode::BlackList => self.matches_blacklist(r),
            Mode::WhiteList => self.matches_whitelist(r),
        }
    }

    pub fn matches_whitelist(&self, r: &InboundInfo) -> bool {
        let ip_is_in = self.is_in_ips(&r.target_addr);
        if !ip_is_in {
            return false;
        }
        true
    }

    pub fn matches_blacklist(&self, r: &InboundInfo) -> bool {
        let ip_is_in = self.is_in_ips(&r.target_addr);
        if ip_is_in {
            return true;
        }

        false
    }

    pub fn is_in_ips(&self, addr: &net::Addr) -> bool {
        match addr.addr {
            NetAddr::Socket(so) | NetAddr::NameAndSocket(_, so, _) => match so {
                std::net::SocketAddr::V4(so) => self.ipv4.contains(so.ip()),
                std::net::SocketAddr::V6(so) => self.ipv6.contains(so.ip()),
            },
            NetAddr::Name(_, _) => false,
        }
    }
}
