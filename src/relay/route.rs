/*!
 * route 模块定义了一些 如何由inbound 的各种信息判断应该选哪个 outbound 作为出口 的方法
 */
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::Arc,
};

use async_trait::async_trait;
use rustls::pki_types::IpAddr;

use crate::{
    map::{AnyBox, AnyData, AnyS, MIterBox},
    net,
    user::{self, User, UserVec},
};

/// Send + Sync to use in async
#[async_trait]
pub trait OutSelector: Send + Sync {
    async fn select(
        &self,
        addr: &net::Addr,
        in_chain_tag: &str,
        params: &Vec<Option<AnyData>>,
    ) -> MIterBox;
}

#[derive(Debug)]
pub struct FixedOutSelector {
    pub default: MIterBox,
}

#[async_trait]
impl OutSelector for FixedOutSelector {
    async fn select(
        &self,
        _addr: &net::Addr,
        _in_chain_tag: &str,
        _params: &Vec<Option<AnyData>>,
    ) -> MIterBox {
        self.default.clone()
    }
}

#[derive(Debug)]
pub struct TagOutSelector {
    pub outbounds_tag_route_map: HashMap<String, String>, // in_tag -> out_tag
    pub outbounds_map: Arc<HashMap<String, MIterBox>>,    //out_tag -> outbound
    pub default: MIterBox,
}

#[async_trait]
impl OutSelector for TagOutSelector {
    async fn select(
        &self,
        _addr: &net::Addr,
        in_chain_tag: &str,
        _params: &Vec<Option<AnyData>>,
    ) -> MIterBox {
        let ov = self.outbounds_tag_route_map.get(in_chain_tag);
        match ov {
            Some(out_k) => {
                let y = self.outbounds_map.get(out_k);
                match y {
                    Some(out) => out.clone(),
                    None => self.default.clone(),
                }
            }
            None => self.default.clone(),
        }
    }
}

#[derive(Hash, Debug, PartialEq, Eq)]
pub struct Rule {
    pub in_tag: String,
    pub target_addr: net::Addr,

    ///因为链中可能有多个用户验证，所以会有多个 UserBox
    pub users: Option<UserVec>,
}

#[derive(Hash, Debug)]
pub struct RouteRuleConfig {
    pub out_tag: String,
    pub users_es: Vec<user::UserVec>,
    pub in_tags: Vec<String>,
    pub ips: Vec<IpAddr>,
    pub domains: Vec<String>,
    pub networks: Vec<String>,
    pub countries: Vec<String>,
}

impl RouteRuleConfig {
    pub fn matches(&self, _r: Rule) -> bool {
        unimplemented!()
    }
}

/// (k,v), v 为 out_tag, k 为 所有能对应 v的 rule 值的集合
#[derive(Debug)]
pub struct RuleSet(HashSet<Rule>, String);
impl RuleSet {
    pub fn matches(&self, r: &Rule) -> Option<String> {
        self.0.get(r).map(|_| self.1.to_string())
    }
}

/// 一种使用 Vec<RuleSet> 的 OutSelector 的实现
#[derive(Debug)]
pub struct RuleSetOutSelector {
    pub outbounds_ruleset_vec: Vec<RuleSet>, // rule -> out_tag
    pub outbounds_map: Arc<HashMap<String, MIterBox>>, //out_tag -> outbound
    pub default: MIterBox,
}

/// from &AnyS get `Box<dyn User>`
///
/// # Example
///
/// ```
/// use ruci::map::AnyBox;
/// use ruci::user::{PlainText, User};
/// use ruci::relay::route::bget_user_from_anydata;
///
/// let u = PlainText::new("u".to_string(), "".to_string());
/// let ub0: Box<dyn User> = Box::new(u);
/// let ub2: AnyArc = Arc::new(Mutex::new(ub0));
/// let anyv = ub2.lock().await;
/// let y = get_user_from_anydata(&*anyv);
/// assert!(y.is_some());
/// ```
///
pub fn get_user_from_anydata(anys: &AnyS) -> Option<Box<dyn User>> {
    let a = anys.downcast_ref::<Box<dyn User>>();
    a.map(|u| u.clone())
}

/// from &AnyBox get `Box<dyn User>`
///
/// # Example
///
/// ```
/// use ruci::map::AnyBox;
/// use ruci::user::{PlainText, User};
/// use ruci::relay::route::bget_user_from_anydata;
///
/// let u = PlainText::new("u".to_string(), "".to_string());
/// let ub0: Box<dyn User> = Box::new(u);
/// let ub2:AnyBox = Box::new(ub0);
/// let y = bget_user_from_anydata(&ub2);
/// assert!(y.is_some());
/// ```
///
pub fn bget_user_from_anydata(anys: &AnyBox) -> Option<Box<dyn User>> {
    let a = anys.downcast_ref::<Box<dyn User>>();
    a.map(|u| u.clone())
}

pub async fn get_user_from_anydata_vec(adv: &Vec<Option<AnyData>>) -> Option<UserVec> {
    let mut v = UserVec::new();

    for anyd in adv
        .iter()
        .filter(|d| d.is_some())
        .map(|d| d.as_ref().unwrap())
    {
        match anyd {
            AnyData::A(arc) => {
                let anyv = arc.lock().await;
                let oub = get_user_from_anydata(&*anyv);
                if let Some(ub) = oub {
                    v.0.push(user::UserBox(ub));
                }
            }
            AnyData::B(b) => {
                let oub = bget_user_from_anydata(b);
                if let Some(ub) = oub {
                    v.0.push(user::UserBox(ub));
                }
            }
            _ => {}
        }
    }
    if v.0.is_empty() {
        None
    } else {
        Some(v)
    }
}

#[async_trait]
impl OutSelector for RuleSetOutSelector {
    async fn select(
        &self,
        addr: &net::Addr,
        in_chain_tag: &str,
        params: &Vec<Option<AnyData>>,
    ) -> MIterBox {
        let users = get_user_from_anydata_vec(params).await;
        let r = Rule {
            in_tag: in_chain_tag.to_string(),
            target_addr: addr.clone(),
            users,
        };
        let mut out_tag: Option<String> = None;
        for rs in self.outbounds_ruleset_vec.iter() {
            if let Some(s) = rs.matches(&r) {
                out_tag = Some(s);
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
    }
}

#[cfg(test)]
mod test {

    use crate::map::math::*;
    use crate::map::*;
    use crate::net::Addr;
    use crate::user::PlainText;

    use super::*;

    fn get_miter_ab() -> MIterBox {
        let mut a = Adder::default();
        a.addnum = 1;
        let a: MapperBox = Box::new(a);

        let b = Adder::default();
        let b: MapperBox = Box::new(b);

        let v = vec![a, b];
        let v = Box::leak(Box::new(v));
        let m: MIterBox = Box::new(v.iter());
        m
    }
    fn get_miter_a() -> MIterBox {
        let mut a = Adder::default();
        a.addnum = 2;
        let a: MapperBox = Box::new(a);

        let v = vec![a];
        let v = Box::leak(Box::new(v));
        let m: MIterBox = Box::new(v.iter());
        m
    }

    #[tokio::test]
    async fn tag_select() {
        let pair_list = vec![
            ("l1".to_string(), "d1".to_string()),
            ("l2".to_string(), "d2".to_string()),
        ];
        let outbounds_route_map: HashMap<_, _> = pair_list.into_iter().collect();

        let m: MIterBox = get_miter_ab();
        let m2: MIterBox = get_miter_a();

        let mut outbounds_map = HashMap::new();
        outbounds_map.insert("d1".to_string(), m);
        let outbounds_map = Arc::new(outbounds_map);

        let t = TagOutSelector {
            outbounds_tag_route_map: outbounds_route_map,
            outbounds_map,
            default: m2,
        };
        let x = t.select(&Addr::default(), "l1", &Vec::new()).await;
        println!("{:?}", x);
        assert_eq!(x.count(), 2);
        let x = t.select(&Addr::default(), "l11", &Vec::new()).await;
        println!("{:?}", x);
        assert_eq!(x.count(), 1);
    }

    #[tokio::test]
    async fn ruleset_select() {
        let m: MIterBox = get_miter_ab();
        let m2: MIterBox = get_miter_a();

        let mut outbounds_map = HashMap::new();
        outbounds_map.insert("d1".to_string(), m);
        let outbounds_map = Arc::new(outbounds_map);

        let r1 = Rule {
            in_tag: String::from("l1"),
            target_addr: Addr::default(),
            users: None,
        };
        let r2 = Rule {
            in_tag: String::from("l2"),
            target_addr: Addr::default(),
            users: None,
        };
        let mut hs = HashSet::new();
        hs.insert(r1);
        hs.insert(r2);

        let rs = RuleSet(hs, "d1".to_string());

        let rsv = vec![rs];

        let rsos = RuleSetOutSelector {
            outbounds_ruleset_vec: rsv,
            outbounds_map,
            default: m2,
        };

        let u = PlainText::new("user".to_string(), "pass".to_string());
        let ub: Box<dyn User> = Box::new(u);

        let mut params: Vec<Option<AnyData>> = Vec::new();
        params.push(Some(AnyData::B(Box::new(ub))));

        let x = rsos.select(&Addr::default(), "l1", &params).await;

        assert_eq!(x.count(), 1);

        params.clear();
        let x = rsos.select(&Addr::default(), "l1", &params).await;

        assert_eq!(x.count(), 2);
    }
}
