use std::collections::HashMap;

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
    pub outbounds_map: HashMap<String, MIterBox>,
    pub default: MIterBox,
}
impl OutSelector for TagOutSelector {
    fn select(&self, in_chain_tag: &str, _params: Vec<Option<AnyData>>) -> MIterBox {
        let ov = self.outbounds_map.get(in_chain_tag);
        match ov {
            Some(v) => v.clone(),
            None => self.default.clone(),
        }
    }
}
