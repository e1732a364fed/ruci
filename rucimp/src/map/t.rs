/*!
 * Tproxy related Mapper
 */
use async_trait::async_trait;
use ruci::map::{self, *};
use ruci::{net::*, Name};

use macro_mapper::{mapper_ext_fields, MapperExt};

/// TproxyResolver 从 系统发来的 tproxy 相关的 连接
/// 解析出实际 target_addr
#[mapper_ext_fields]
#[derive(Debug, Clone, Default, MapperExt)]
pub struct TproxyResolver {}

impl Name for TproxyResolver {
    fn name(&self) -> &'static str {
        "tproxy_resolver"
    }
}

#[async_trait]
impl Mapper for TproxyResolver {
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
                let (_, laddr) = match ap {
                    AnyData::RLAddr(a) => a,
                    _ => return MapResult::err_str("Tproxy needs RLAddr , got other data."),
                };

                // laddr in tproxy is in fact target_addr
                MapResult::newc(c).a(Some(laddr)).build()
            }
            Stream::AddrConn(_) => todo!(),
            _ => MapResult::err_str(&format!("Tproxy needs a stream, got {}", params.c)),
        }
    }
}
