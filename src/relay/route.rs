use crate::map::{AnyData, MIterBox};

/// Send + Sync to use in async
pub trait OutSelector: Send + Sync {
    fn select(&self, in_chain_tag: &str, params: Vec<Option<AnyData>>) -> MIterBox;
}

pub struct FixedOutSelector {
    pub mappers: MIterBox,
}

impl OutSelector for FixedOutSelector {
    fn select(&self, _in_chain_tag: &str, _params: Vec<Option<AnyData>>) -> MIterBox {
        self.mappers.clone()
    }
}
