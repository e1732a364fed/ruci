/*!
 * route 模块定义了一些 如何由inbound 的各种信息判断应该选哪个 outbound 作为出口 的方法
 */
use std::{collections::HashMap, sync::Arc};

use crate::{
    map::{AnyData, MIterBox},
    net,
    user::{self},
};

/// Send + Sync to use in async
pub trait OutSelector: Send + Sync {
    fn select(
        &self,
        addr: &net::Addr,
        in_chain_tag: &str,
        params: Vec<Option<AnyData>>,
    ) -> MIterBox;
}

#[derive(Debug)]
pub struct FixedOutSelector {
    pub default: MIterBox,
}

impl OutSelector for FixedOutSelector {
    fn select(
        &self,
        _addr: &net::Addr,
        _in_chain_tag: &str,
        _params: Vec<Option<AnyData>>,
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

impl OutSelector for TagOutSelector {
    fn select(
        &self,
        _addr: &net::Addr,
        in_chain_tag: &str,
        _params: Vec<Option<AnyData>>,
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

pub struct RouteRule {
    pub users: Box<dyn user::UserTrait>,
}

#[derive(Debug)]
pub struct RuleOutSelector {
    pub outbounds_tag_route_map: HashMap<String, String>, // in_tag -> out_tag
    pub outbounds_map: Arc<HashMap<String, MIterBox>>,    //out_tag -> outbound
    pub default: MIterBox,
}

#[cfg(test)]
mod test {
    use crate::map::math::*;
    use crate::map::*;
    use crate::net::Addr;

    use super::*;
    #[test]
    fn tag_select() {
        let teams_list = vec![
            ("l1".to_string(), "d1".to_string()),
            ("l2".to_string(), "d2".to_string()),
        ];
        let outbounds_route_map: HashMap<_, _> = teams_list.into_iter().collect();

        let a = Adder::default();
        let a: MapperBox = Box::new(a);

        let ac = a.clone();

        let b = Adder::default();
        let b: MapperBox = Box::new(b);

        let v = vec![a, b];
        let v2 = vec![ac];
        let v = Box::leak(Box::new(v));
        let m: MIterBox = Box::new(v.iter());

        let v2 = Box::leak(Box::new(v2));
        let m2: MIterBox = Box::new(v2.iter());

        let mut outbounds_map = HashMap::new();
        outbounds_map.insert("d1".to_string(), m);
        let outbounds_map = Arc::new(outbounds_map);

        let t = TagOutSelector {
            outbounds_tag_route_map: outbounds_route_map,
            outbounds_map,
            default: m2,
        };
        let x = t.select(&Addr::default(), "l1", Vec::new());
        println!("{:?}", x);
        assert_eq!(x.count(), 2);
        let x = t.select(&Addr::default(), "l11", Vec::new());
        println!("{:?}", x);
        assert_eq!(x.count(), 1);
    }
}
