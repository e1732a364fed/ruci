use async_trait::async_trait;
use macro_map::*;
use ruci::{
    map::{self, Map, MapParams, MapResult, ProxyBehavior},
    net::{Addr, CID},
    Name,
};

#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct IpTest1 {}

impl Name for IpTest1 {
    fn name(&self) -> &'static str {
        "ip_relay_test1"
    }
}

#[async_trait]
impl Map for IpTest1 {
    async fn maps(&self, _cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match behavior {
            ProxyBehavior::UNSPECIFIED => panic!("impossible"),
            ProxyBehavior::ENCODE => MapResult::builder()
                .a(params.a)
                .b(params.b)
                .c(params.c)
                .build(),
            ProxyBehavior::DECODE => MapResult::builder()
                .a(Some(Addr::from_network_addr_url("ip://0.0.0.0").unwrap()))
                .b(params.b)
                .c(params.c)
                .build(),
        }
    }
}
