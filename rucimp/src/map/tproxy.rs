/*!
Tproxy related Mapper. Tproxy is shortcut for transparent proxy,

Only support linux
 */
use async_trait::async_trait;
use ruci::map::{self, *};
use ruci::{net::*, Name};

use macro_mapper::{mapper_ext_fields, MapperExt};
use tracing::debug;

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

fn get_laddr_from_vd(vd: Vec<Option<Box<dyn Data>>>) -> Option<ruci::net::Addr> {
    for vd in vd.iter().flatten() {
        let oa = vd.get_laddr();
        if oa.is_some() {
            return oa;
        }
    }
    None
}

#[async_trait]
impl Mapper for TproxyResolver {
    ///tproxy only has decode behavior
    ///
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::Conn(c) => {
                let oa = get_laddr_from_vd(params.d);

                if oa.is_none() {
                    return MapResult::err_str(
                        "Tproxy needs data for local_addr, did't get it from the data.",
                    );
                }
                debug!(cid = %cid, a=?oa, "tproxy got target_addr: ");

                // laddr in tproxy is in fact target_addr
                MapResult::new_c(c).a(oa).b(params.b).build()
            }
            Stream::AddrConn(_) => todo!(),
            _ => MapResult::err_str(&format!("Tproxy needs a stream, got {}", params.c)),
        }
    }
}
