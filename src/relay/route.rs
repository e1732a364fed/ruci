/*!
 * route 模块定义了一些 如何由inbound 的各种信息判断应该选哪个 outbound 作为出口 的方法

它被一些代理称为 ACL (Access Control List), 但这个名称并不准确, "路由规则"更加准确. 因为
不仅可以用于 "防火墙", 还可以用于分流

因为本模块属于 ruci 包, 所以这里只实现一些简易通用的 OutSelector, 复杂的需要外部依赖包的实现 需要在其它包中实现。

也因为, 复杂的规则往往有自定义的配置格式, 而ruci包是 配置无关的.


trait: OutSelector

impl: FixedOutSelector, TagOutSelector, InboundInfoOutSelector

struct: Rule, RuleSet



 */

use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::Arc,
};

use async_trait::async_trait;

use crate::{
    map::acc::DMIterBox,
    net,
    user::{self, UserVec},
};

use super::Data;

/// Send + Sync to use in async
///
/// OutSelector 给了 从一次链累加行为中 得到的数据 来试图 选择出一个 DMIterBox
///
/// 选择出的 DMIterBox 一般是用于 outbound,
///
/// params 的类型 是链累加 得到的 结果, 里面可能有任何值,
///
/// 不过对于路由来说最有用的值应该是 验证后的 user, 所以本模块中含一个 get_user_from_optdatas
/// 函数用于这一点.
///
#[async_trait]
pub trait OutSelector: Send + Sync {
    async fn select(
        &self,
        addr: &net::Addr,
        in_chain_tag: &str,
        params: &[Option<Box<dyn Data>>],
    ) -> DMIterBox;
}

pub async fn get_user_from_optdatas(adv: &[Option<Box<dyn Data>>]) -> Option<UserVec> {
    let mut v = UserVec::default();

    for anyd in adv.iter().flatten() {
        if let Some(u) = anyd.get_user() {
            v.0.push(user::UserBox(u.clone()));
        }
    }
    if v.0.is_empty() {
        None
    } else {
        Some(v)
    }
}

#[derive(Debug)]
pub struct FixedOutSelector {
    pub default: DMIterBox,
}

#[async_trait]
impl OutSelector for FixedOutSelector {
    async fn select(
        &self,
        _addr: &net::Addr,
        _in_chain_tag: &str,
        _params: &[Option<Box<dyn Data>>],
    ) -> DMIterBox {
        self.default.clone()
    }
}

#[derive(Debug)]
pub struct TagOutSelector {
    pub outbounds_tag_route_map: HashMap<String, String>, // in_tag -> out_tag
    pub outbounds_map: Arc<HashMap<String, DMIterBox>>,   //out_tag -> outbound
    pub default: DMIterBox,
}

#[async_trait]
impl OutSelector for TagOutSelector {
    async fn select(
        &self,
        _addr: &net::Addr,
        in_chain_tag: &str,
        _params: &[Option<Box<dyn Data>>],
    ) -> DMIterBox {
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

/// 一种基本的 inbound 部分有用信息的结构, 分别标明:
///
/// 1. 是谁进来的:      users
/// 2. 从哪里进来的:    in_tag
/// 3. 要到哪里去:      target_addr
///
#[derive(Hash, Debug, PartialEq, Eq)]
pub struct InboundInfo {
    ///因为链中可能有多个用户验证，所以会有多个 UserBox
    pub users: Option<UserVec>,
    pub in_tag: String,
    pub target_addr: net::Addr,
}

/// (k,v), v 为 out_tag, k 为 所有能对应 v的 rule 值的集合
#[derive(Debug)]
pub struct InboundInfoOutTagPair(HashSet<InboundInfo>, String);
impl InboundInfoOutTagPair {
    pub fn matches(&self, r: &InboundInfo) -> Option<String> {
        self.0.get(r).map(|_| self.1.to_string())
    }
}

/// 一种使用 Vec<InboundInfoOutTagPair> 的 OutSelector 的实现
///
/// 仅用于 InboundInfoOutTagPair 很少 且每个 InboundInfoOutTagPair 中的
/// HashSet<InboundInfo> 中的 InboundInfo
/// 都很少的情况, 即 适用于精确匹配
///
#[derive(Debug)]
pub struct InboundInfoOutSelector {
    pub outbounds_ruleset_vec: Vec<InboundInfoOutTagPair>, // rule -> out_tag
    pub outbounds_map: Arc<HashMap<String, DMIterBox>>,    //out_tag -> outbound
    pub default: DMIterBox,
}
#[async_trait]
impl OutSelector for InboundInfoOutSelector {
    async fn select(
        &self,
        addr: &net::Addr,
        in_chain_tag: &str,
        params: &[Option<Box<dyn Data>>],
    ) -> DMIterBox {
        let users = get_user_from_optdatas(params).await;
        let r = InboundInfo {
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

    use self::acc::DynVecIterWrapper;

    use super::*;

    fn get_miter_ab() -> DMIterBox {
        let mut a = Adder::default();
        a.addnum = 1;
        let a: MapperBox = Box::new(a);

        let b = Adder::default();
        let b: MapperBox = Box::new(b);

        let v = vec![a, b];
        let v: Vec<_> = v.into_iter().map(|b| Arc::new(b)).collect();
        // let m: DMIterBox = Box::new(DynMIterWrapper(Box::new(v.into_iter())));
        let m: DMIterBox = Box::new(DynVecIterWrapper(v.into_iter()));
        m
    }
    fn get_miter_a() -> DMIterBox {
        let mut a = Adder::default();
        a.addnum = 2;
        let a: MapperBox = Box::new(a);

        let v = vec![a];
        let v: Vec<_> = v.into_iter().map(|b| Arc::new(b)).collect();
        //let m: DMIterBox = Box::new(DynMIterWrapper(Box::new(v.into_iter())));
        let m: DMIterBox = Box::new(DynVecIterWrapper(v.into_iter()));

        m
    }

    #[tokio::test]
    async fn test_tag_select() {
        let pair_list = vec![
            ("l1".to_string(), "d1".to_string()),
            ("l2".to_string(), "d2".to_string()),
        ];
        let outbounds_route_map: HashMap<_, _> = pair_list.into_iter().collect();

        let m: DMIterBox = get_miter_ab();
        let m2: DMIterBox = get_miter_a();

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
        assert_eq!(x.get_miter().unwrap().count(), 2); //can't count DMIter directly
        let x = t.select(&Addr::default(), "l11", &Vec::new()).await;
        println!("{:?}", x);
        assert_eq!(x.get_miter().unwrap().count(), 1);
    }

    #[tokio::test]
    async fn test_inbound_info_select() {
        let m: DMIterBox = get_miter_ab();
        let m2: DMIterBox = get_miter_a();

        let mut outbounds_map = HashMap::new();
        outbounds_map.insert("d1".to_string(), m);
        let outbounds_map = Arc::new(outbounds_map);

        let r1 = InboundInfo {
            in_tag: String::from("l1"),
            target_addr: Addr::default(),
            users: None,
        };
        let r2 = InboundInfo {
            in_tag: String::from("l2"),
            target_addr: Addr::default(),
            users: None,
        };
        let mut hs = HashSet::new();
        hs.insert(r1);
        hs.insert(r2);

        let rs = InboundInfoOutTagPair(hs, "d1".to_string());

        let rsv = vec![rs];

        let rsos = InboundInfoOutSelector {
            outbounds_ruleset_vec: rsv,
            outbounds_map,
            default: m2,
        };

        let u = PlainText::new("user".to_string(), "pass".to_string());
        //let ub: Box<dyn User> = Box::new(u);

        let mut params: Vec<Option<Box<dyn Data>>> = Vec::new();
        params.push(Some(Box::new(u)));

        let x = rsos.select(&Addr::default(), "l1", &params).await;

        assert_eq!(x.get_miter().unwrap().count(), 1);

        println!("{:?}", x);
        params.clear();
        let x = rsos.select(&Addr::default(), "l1", &params).await;

        assert_eq!(x.get_miter().unwrap().count(), 2);
        println!("{:?}", x);
    }
}
