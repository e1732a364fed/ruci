use std::{collections::HashMap, sync::Arc};

use crate::map::{AnyData, MIterBox};

/// Send + Sync to use in async
pub trait OutSelector: Send + Sync {
    fn select(&self, in_chain_tag: &str, params: Vec<Option<AnyData>>) -> MIterBox;
}

pub struct FixedOutSelector {
    pub default: MIterBox,
}

impl OutSelector for FixedOutSelector {
    fn select(&self, _in_chain_tag: &str, _params: Vec<Option<AnyData>>) -> MIterBox {
        self.default.clone()
    }
}

pub struct TagOutSelector {
    pub outbounds_route_map: HashMap<String, String>, // in -> out
    pub outbounds_map: Arc<HashMap<String, MIterBox>>, //out_tag -> outbound
    pub default: MIterBox,
}

impl OutSelector for TagOutSelector {
    fn select(&self, in_chain_tag: &str, _params: Vec<Option<AnyData>>) -> MIterBox {
        let ov = self.outbounds_route_map.get(in_chain_tag);
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
