/*!
* WhiteList 和 BlackList 两种模式

*/
use std::collections::HashSet;

use ipnet::*;
use iprange::IpRange;
use regex::RegexSet;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Mode {
    BlackList,
    WhiteList,
}

#[derive(Clone, Debug)]
pub struct Rules {
    pub ipv4: IpRange<Ipv4Net>,
    pub ipv6: IpRange<Ipv6Net>,
    pub rule_regex: RegexSet,
    pub rule_set: HashSet<String>,
}
