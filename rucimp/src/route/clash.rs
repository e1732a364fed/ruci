/*! implement clash rules using crate clash_rule
*/
use ruci::map::fold::DMIterBox;
use ruci::map::Data;
use ruci::net;
use ruci::relay::route;

use clash_rules::*;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ClashRuleOutSelector {
    pub matcher: ClashRuleMatcher,
    pub outbounds_map: Arc<HashMap<String, DMIterBox>>, //out_tag -> outbound
    pub default: DMIterBox,
}
#[async_trait]
impl route::OutSelector for ClashRuleOutSelector {
    async fn select(
        &self,
        is_fallback: bool,
        addr: &net::Addr,
        in_chain_tag: &str,
        params: &[Option<Box<dyn Data>>],
    ) -> Option<DMIterBox> {
        let mut out_tag: Option<&String> = None;
        if let Some(ip) = addr.get_ip() {
            out_tag = self.matcher.check_ip(ip);
        }
        if out_tag.is_none() {
            if let Some(d) = addr.get_name() {
                out_tag = self.matcher.check_domain(&d);
            }
        }
        let r = match out_tag {
            None => self.default.clone(),
            Some(out_k) => {
                let y = self.outbounds_map.get(&out_k);
                match y {
                    Some(out) => out.clone(),
                    None => self.default.clone(),
                }
            }
        };

        Some(r)
    }
}
