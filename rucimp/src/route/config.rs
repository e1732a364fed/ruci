use std::collections::HashSet;

use ipnet::{Ipv4Net, Ipv6Net};
use iprange::IpRange;
use itertools::Itertools;
use regex::RegexSet;
use ruci::{
    net::Network,
    user::{UserBox, UserVec},
};
use serde::{Deserialize, Serialize};

use crate::user::str_to_userbox;

use super::{DomainMatcher, Mode, RuleSet};

/// matches the structure of rucimp::route::RuleSet, and provide  serde
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuleSetConfig {
    pub out_tag: String,

    pub mode: ModeConfig,
    pub in_tags: Option<HashSet<String>>,

    pub userset: Option<HashSet<Vec<String>>>,

    pub ta_ip_countries: Option<HashSet<String>>,

    pub ta_networks: Option<HashSet<String>>,

    pub ta_ipv4: Option<Vec<String>>,
    pub ta_ipv6: Option<Vec<String>>,

    pub ta_domain_matcher: Option<DomainMatcherConfig>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Default, Serialize, Deserialize)]
pub enum ModeConfig {
    BlackList,

    #[default]
    WhiteList,
}
impl ModeConfig {
    pub fn to_mode(&self) -> Mode {
        match self {
            ModeConfig::BlackList => Mode::BlackList,
            ModeConfig::WhiteList => Mode::WhiteList,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomainMatcherConfig {
    pub domain_regex: Option<Vec<String>>,
    pub domain_set: Option<HashSet<String>>,
}
impl DomainMatcherConfig {
    pub fn to_dm(self) -> DomainMatcher {
        let dr = self.domain_regex.map(|dr| RegexSet::new(dr).unwrap());

        DomainMatcher {
            domain_regex: dr,
            domain_set: self.domain_set,
        }
    }
}

impl RuleSetConfig {
    pub fn to_ruleset(self) -> RuleSet {
        let userset = self.userset.map(|uss| {
            let y: HashSet<UserVec> = uss
                .iter()
                .map(|us_v| {
                    let z: Vec<UserBox> =
                        us_v.iter().map(|us| str_to_userbox(us).unwrap()).collect();
                    UserVec(z)
                })
                .collect();
            y
        });

        let netset = self.ta_networks.map(|hm| {
            let hs: HashSet<Network> = hm
                .iter()
                .map(|ns| Network::from_string(&ns).unwrap_or_default())
                .dedup()
                .collect();

            hs
        });

        let ip4 = self.ta_ipv4.map(|ip4| {
            let ip4: IpRange<Ipv4Net> = ip4.iter().map(|s| s.parse().unwrap()).collect();

            ip4
        });

        let ip6 = self.ta_ipv6.map(|ip6| {
            let ip6: IpRange<Ipv6Net> = ip6.iter().map(|s| s.parse().unwrap()).collect();

            ip6
        });

        let dm = self.ta_domain_matcher.map(|dm| dm.to_dm());

        RuleSet {
            out_tag: self.out_tag,
            mode: self.mode.to_mode(),
            userset,
            in_tags: self.in_tags,
            ta_ip_countries: self.ta_ip_countries,
            ta_networks: netset,
            ta_ipv4: ip4,
            ta_ipv6: ip6,
            ta_domain_matcher: dm,
            ..Default::default()
        }
    }
}
