/*!
 * Tproxy related Mapper. t is shortcut for transparent proxy,
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

/// 同时传入 raddr 和 laddr 时, raddr 在前 , laddr 在后,
/// 所以要 逆序查找 第一个 addr 才是 laddr
fn get_addr_from_vvd_rev(vd: Vec<Option<VecAnyData>>) -> Option<ruci::net::Addr> {
    for ovd in vd.iter() {
        match ovd {
            Some(vd) => match vd {
                VecAnyData::Data(d) => return ruci::map::network::get_addr_from_d(d),
                VecAnyData::Vec(vd) => {
                    for x in vd.iter().rev() {
                        let oa = ruci::map::network::get_addr_from_d(x);
                        if oa.is_some() {
                            return oa;
                        }
                    }
                }
            },
            None => return None,
        }
    }
    None
}

#[async_trait]
impl Mapper for TproxyResolver {
    ///tproxy only has decode behavior
    ///
    async fn maps(&self, _cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::Conn(c) => {
                let oa = get_addr_from_vvd_rev(params.d);
                // let ap = match params.d {
                //     Some(a) => a,
                //     None => {
                //         return MapResult::err_str(
                //             "Tproxy needs data for local_addr, got None data.",
                //         )
                //     }
                // };
                // let (_, laddr) = match ap {
                //     AnyData::RLAddr(a) => a,
                //     _ => return MapResult::err_str("Tproxy needs RLAddr , got other data."),
                // };
                if oa.is_none() {
                    return MapResult::err_str(
                        "Tproxy needs data for local_addr, did't get it from the data.",
                    );
                }

                // laddr in tproxy is in fact target_addr
                MapResult::newc(c).a(oa).build()
            }
            Stream::AddrConn(_) => todo!(),
            _ => MapResult::err_str(&format!("Tproxy needs a stream, got {}", params.c)),
        }
    }
}
