use async_trait::async_trait;
use ruci::map::{self, *};
use ruci::{net::*, Name};

use macro_mapper::{mapper_ext_fields, MapperExt};

#[mapper_ext_fields]
#[derive(Debug, Clone, Default, MapperExt)]
pub struct Tproxy {}

impl Name for Tproxy {
    fn name(&self) -> &'static str {
        "tproxy"
    }
}

#[async_trait]
impl Mapper for Tproxy {
    ///tproxy only has decode behavior
    ///
    async fn maps(&self, _cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::Conn(c) => {
                let ap = match params.d {
                    Some(a) => a,
                    None => {
                        return MapResult::err_str(
                            "Tproxy needs data for local_addr, got None data.",
                        )
                    }
                };
                let (raddr, laddr) = match ap {
                    AnyData::RLAddr(a) => a,
                    _ => return MapResult::err_str("Tproxy needs RLAddr , got other data."),
                };
                MapResult::newc(c).build()
            }
            Stream::AddrConn(_) => todo!(),
            _ => MapResult::err_str(&format!("Tproxy needs a stream, got {}", params.c)),
        };
        unimplemented!()
    }
}
