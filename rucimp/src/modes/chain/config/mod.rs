/*!
 * 定义了静态链式配置 StaticConfig
 * 静态链是Mapper组成是运行前即知晓且依次按排列顺序执行的链, 因此可以用 Vec 表示
 *
 * 有限动态链的Mapper组成也可用 StaticConfig 定义, 但其状态转移函数在dynamic模块中定义
 */

#[cfg(feature = "lua")]
pub mod lua;

pub mod dynamic;

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use bytes::BytesMut;
use ruci::{
    map::{
        counter::Counter,
        fold::{DMIterBox, DynVecIterWrapper},
        network::{echo::Echo, BlackHole, Direct},
        *,
    },
    net::{self, http::CommonConfig},
};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::map::ws;
#[cfg(feature = "route")]
use crate::route::{config::RuleSetConfig, RuleSet};

/// 静态配置中有初始化后即确定的 Mapper 数量
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct StaticConfig {
    pub inbounds: Vec<InMapperConfigChain>,
    pub outbounds: Vec<OutMapperConfigChain>,

    pub tag_route: Option<Vec<(String, String)>>,
    pub fallback_route: Option<Vec<(String, String)>>,

    #[cfg(feature = "route")]
    pub rule_route: Option<Vec<RuleSetConfig>>,
}

impl StaticConfig {
    /// convert config chain to mapper chain
    pub fn get_inbounds(&self) -> Vec<Vec<MapperBox>> {
        let listens: Vec<_> = self
            .inbounds
            .iter()
            .map(|config_chain| {
                let mut chain = config_chain
                    .chain
                    .iter()
                    .map(|mapper_config| {
                        let mut mapper = mapper_config.to_mapper_box();
                        mapper.set_chain_tag(config_chain.tag.as_deref().unwrap_or(""));
                        mapper
                    })
                    .collect::<Vec<_>>();

                if let Some(last_m) = chain.last_mut() {
                    last_m.set_is_tail_of_chain(true);
                } else {
                    warn!("the inbound chain has no mappers, {:?}", config_chain.tag);
                }

                chain
            })
            .collect();

        listens
    }

    /// convert config chain to mapper chain
    pub fn get_outbounds(&self) -> Vec<Vec<MapperBox>> {
        self.outbounds
            .iter()
            .map(|config_chain| {
                let mut chain = config_chain
                    .chain
                    .iter()
                    .map(|mapper_config| {
                        let mut mapper = mapper_config.to_mapper_box();
                        mapper.set_chain_tag(&config_chain.tag);
                        mapper
                    })
                    .collect::<Vec<_>>();

                if let Some(last_m) = chain.last_mut() {
                    last_m.set_is_tail_of_chain(true);
                } else {
                    warn!("the outbound chain has no mappers, {:?}", config_chain.tag);
                }

                chain
            })
            .collect::<Vec<_>>()
    }

    /// (out_tag, outbound)
    pub fn get_default_and_outbounds_map(&self) -> (DMIterBox, HashMap<String, DMIterBox>) {
        let obs = self.get_outbounds();

        let mut first_o: Option<DMIterBox> = None;

        let o_map = obs
            .into_iter()
            .map(|outbound| {
                let tag = outbound
                    .first()
                    .expect("outbound should has at least one mapper ")
                    .get_chain_tag();

                let ts = tag.to_string();
                let outbound: Vec<_> = outbound.into_iter().map(Arc::new).collect();

                let outbound_iter: DMIterBox = Box::new(DynVecIterWrapper(outbound.into_iter()));

                if first_o.is_none() {
                    first_o = Some(outbound_iter.clone());
                }

                (ts, outbound_iter)
            })
            .collect();
        (first_o.expect("has an outbound"), o_map)
    }

    /// panic if the given tag isn't presented in outbounds
    pub fn get_tag_route(&self) -> Option<HashMap<String, String>> {
        self.tag_route.as_ref().map(|tr| {
            let route_tag_pairs = tr.clone();
            route_tag_pairs.into_iter().collect::<HashMap<_, _>>()
        })
    }

    pub fn get_fallback_route(&self) -> Option<HashMap<String, String>> {
        self.fallback_route.as_ref().map(|tr| {
            let route_tag_pairs = tr.clone();
            route_tag_pairs.into_iter().collect::<HashMap<_, _>>()
        })
    }

    #[cfg(feature = "route")]
    pub fn get_rule_route(&self) -> Option<Vec<RuleSet>> {
        let mut result = self.rule_route.clone().map(|rr| {
            let x: Vec<RuleSet> = rr.into_iter().map(|r| r.to_rule_set()).collect();
            x
        });
        #[cfg(feature = "geoip")]
        {
            if let Some(mut rs_v) = result {
                use crate::route::maxmind;

                let r = maxmind::open_mmdb("Country.mmdb", &crate::COMMON_DIRS);
                match r {
                    Ok(m) => {
                        let am = Some(Arc::new(m));

                        rs_v.iter_mut().for_each(|rs| rs.mmdb_reader = am.clone());
                    }
                    Err(e) => {
                        warn!("no Country.mmdb: {e}");
                    }
                }

                result = Some(rs_v);
            }
        }
        result
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct InMapperConfigChain {
    tag: Option<String>,
    chain: Vec<InMapperConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct OutMapperConfigChain {
    tag: String, //每个 out chain 都必须有一个 tag
    chain: Vec<OutMapperConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum InMapperConfig {
    Echo,             //单流消耗器
    Stdio(Ext),       //单流发生器
    Fileio(File),     //单流发生器
    Dialer(String),   //单流发生器
    Listener(String), //多流发生器
    Adder(i8),
    Counter,
    TLS(TlsIn),

    #[cfg(feature = "tokio-native-tls")]
    NativeTLS(TlsIn),
    H2 {
        is_grpc: Option<bool>,
        http_config: Option<CommonConfig>,
    },

    Http(PlainTextSet),
    Socks5(PlainTextSet),
    Socks5Http(PlainTextSet),
    Trojan(TrojanPassSet),
    HttpFilter(Option<CommonConfig>),
    WebSocket {
        http_config: Option<CommonConfig>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum OutMapperConfig {
    Blackhole,      //单流消耗器
    Direct,         //单流发生器
    Stdio(Ext),     //单流发生器
    Fileio(File),   //单流发生器
    Dialer(String), //单流发生器
    Adder(i8),
    Counter,
    TLS(TlsOut),

    #[cfg(feature = "tokio-native-tls")]
    NativeTLS(TlsOut),

    Socks5(Socks5Out),
    Trojan(String),
    WebSocket(CommonConfig),
    H2Single,
    H2Mux {
        is_grpc: Option<bool>,

        http_config: Option<CommonConfig>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Ext {
    pub fixed_target_addr: Option<String>,

    pub pre_defined_early_data: Option<String>,
}
impl Ext {
    fn to_ext_fields(&self) -> MapperExtFields {
        let mut ext_f = MapperExtFields::default();
        if let Some(ta) = self.fixed_target_addr.as_ref() {
            ext_f.fixed_target_addr = net::Addr::from_network_addr_str(ta).ok();
        }
        if let Some(s) = self.pre_defined_early_data.as_ref() {
            ext_f.pre_defined_early_data = Some(BytesMut::from(s.as_bytes()));
        }
        ext_f
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct File {
    i: String,
    o: String,

    sleep_interval: Option<u64>,
    bytes_per_turn: Option<usize>,

    ext: Option<Ext>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TlsIn {
    cert: String,
    key: String,
    alpn: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TlsOut {
    host: String,
    insecure: Option<bool>,
    alpn: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlainTextSet {
    userpass: Option<String>,
    more: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Socks5Out {
    userpass: Option<String>,
    early_data: Option<bool>,

    ext: Option<Ext>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrojanPassSet {
    password: Option<String>,
    more: Option<Vec<String>>,
}

impl ToMapperBox for InMapperConfig {
    fn to_mapper_box(&self) -> ruci::map::MapperBox {
        match self {
            InMapperConfig::Echo => Box::<Echo>::default(),
            InMapperConfig::Stdio(ext) => {
                let ext_f = ext.to_ext_fields();

                let mut s = ruci::map::stdio::Stdio::boxed();
                s.set_ext_fields(Some(ext_f));
                s
            }
            InMapperConfig::Fileio(f) => {
                let s = ruci::map::fileio::FileIO {
                    i_name: f.i.clone(),
                    o_name: f.o.clone(),
                    sleep_interval: f.sleep_interval.map(Duration::from_millis),
                    bytes_per_turn: f.bytes_per_turn,
                    ext_fields: f.ext.clone().map(|e| e.to_ext_fields()),
                };
                Box::new(s)
            }
            InMapperConfig::Dialer(td_str) => {
                let a = net::Addr::from_name_network_addr_str(td_str)
                    .expect("network_ip_addr is valid");
                let mut d = ruci::map::network::Dialer::default();
                d.set_configured_target_addr(Some(a));
                Box::new(d)
            }
            InMapperConfig::Listener(l_str) => {
                let a = net::Addr::from_network_addr_str(l_str).expect("network_addr is valid");
                let mut g = ruci::map::network::Listener::default();
                g.set_configured_target_addr(Some(a));
                Box::new(g)
            }
            InMapperConfig::Adder(i) => i.to_mapper_box(),
            InMapperConfig::Counter => Box::<Counter>::default(),
            InMapperConfig::TLS(c) => tls::server::ServerOptions {
                addr: "todo!()".to_string(),
                cert: PathBuf::from(c.cert.clone()),
                key: PathBuf::from(c.key.clone()),
                alpn: c.alpn.clone(),
            }
            .to_mapper_box(),

            #[cfg(feature = "tokio-native-tls")]
            InMapperConfig::NativeTLS(c) => Box::new(
                crate::map::native_tls::ServerOptions {
                    cert_f_path: c.cert.clone(),
                    key_f_path: c.key.clone(),
                }
                .get_server()
                .unwrap(),
            ),

            InMapperConfig::Http(c) => {
                let so = http_proxy::Config {
                    user_whitespace_pass: c.userpass.clone(),
                    user_passes: c.more.as_ref().map(|up_v| {
                        up_v.iter()
                            .map(|up| ruci::user::PlainText::from(up.to_string()))
                            .collect::<Vec<_>>()
                    }),
                    ..Default::default()
                };

                so.to_mapper_box()
            }
            InMapperConfig::Socks5(c) => {
                let so = socks5::server::Config {
                    support_udp: true, //默认打开udp 支持
                    user_whitespace_pass: c.userpass.clone(),
                    user_passes: c.more.as_ref().map(|up_v| {
                        up_v.iter()
                            .map(|up| ruci::user::PlainText::from(up.to_string()))
                            .collect::<Vec<_>>()
                    }),
                };

                so.to_mapper_box()
            }
            InMapperConfig::Socks5Http(c) => {
                let so = socks5http::Config {
                    user_whitespace_pass: c.userpass.clone(),
                    user_passes: c.more.as_ref().map(|up_v| {
                        up_v.iter()
                            .map(|up| ruci::user::PlainText::from(up.to_string()))
                            .collect::<Vec<_>>()
                    }),
                };

                so.to_mapper_box()
            }
            InMapperConfig::Trojan(c) => {
                let so = trojan::server::Config {
                    pass: c.password.clone(),
                    passes: c.more.as_ref().map(|up_v| up_v.to_vec()),
                };

                so.to_mapper_box()
            }
            InMapperConfig::WebSocket {
                http_config: config,
            } => Box::new(crate::map::ws::server::Server {
                config: config.clone(),
            }),
            InMapperConfig::HttpFilter(c) => {
                Box::new(ruci::map::http_filter::Server { config: c.clone() })
            }
            InMapperConfig::H2 {
                http_config: config,
                is_grpc,
            } => Box::new(crate::map::h2::server::Server::new(
                *is_grpc,
                config.clone(),
            )),
        }
    }
}
impl ToMapperBox for OutMapperConfig {
    fn to_mapper_box(&self) -> ruci::map::MapperBox {
        match self {
            OutMapperConfig::Stdio(ext) => {
                let ext_f = ext.to_ext_fields();

                let mut s = ruci::map::stdio::Stdio::boxed();
                s.set_ext_fields(Some(ext_f));
                s
            }
            OutMapperConfig::Fileio(f) => {
                let s = ruci::map::fileio::FileIO {
                    i_name: f.i.clone(),
                    o_name: f.o.clone(),
                    sleep_interval: f.sleep_interval.map(Duration::from_millis),
                    bytes_per_turn: f.bytes_per_turn,
                    ext_fields: f.ext.clone().map(|e| e.to_ext_fields()),
                };
                Box::new(s)
            }
            OutMapperConfig::Blackhole => Box::<BlackHole>::default(),

            OutMapperConfig::Direct => Box::<Direct>::default(),
            OutMapperConfig::Dialer(td_str) => {
                let a = net::Addr::from_name_network_addr_str(td_str)
                    .expect("network_ip_addr is valid");
                let mut d = ruci::map::network::Dialer::default();
                d.set_configured_target_addr(Some(a));
                Box::new(d)
            }
            OutMapperConfig::Adder(i) => i.to_mapper_box(),
            OutMapperConfig::Counter => Box::<counter::Counter>::default(),
            OutMapperConfig::TLS(c) => {
                let a = tls::client::Client::new(tls::client::ClientOptions {
                    domain: c.host.clone(),
                    is_insecure: c.insecure.unwrap_or_default(),
                    alpn: c.alpn.clone(),
                });
                Box::new(a)
            }

            #[cfg(feature = "tokio-native-tls")]
            OutMapperConfig::NativeTLS(c) => Box::new(crate::map::native_tls::Client {
                domain: c.host.clone(),
                in_secure: c.insecure.unwrap_or_default(),
                ext_fields: Some(MapperExtFields::default()),
            }),

            OutMapperConfig::Socks5(c) => {
                let u = c.userpass.clone().unwrap_or_default();
                let mut a = socks5::client::Client {
                    up: if u.is_empty() {
                        None
                    } else {
                        Some(ruci::user::PlainText::from(u))
                    },
                    use_earlydata: c.early_data.unwrap_or_default(),
                };
                if let Some(ext) = &c.ext {
                    a.set_ext_fields(Some(ext.to_ext_fields()))
                }
                Box::new(a)
            }
            OutMapperConfig::Trojan(pass) => {
                let a = trojan::client::Client::new(pass);
                Box::new(a)
            }
            OutMapperConfig::WebSocket(c) => {
                let client = ws::client::Client::new(c.clone());

                Box::new(client)
            }
            OutMapperConfig::H2Single => Box::new(crate::map::h2::client::SingleClient {}),
            OutMapperConfig::H2Mux {
                http_config: config,
                is_grpc,
            } => {
                let m = crate::map::h2::client::MuxClient::new(
                    is_grpc.unwrap_or_default(),
                    config.clone(),
                );

                Box::new(m)
            }
        }
    }
}

#[cfg(test)]
mod test {

    use super::*;
    #[test]
    fn serialize_toml() {
        let sc = StaticConfig {
            inbounds: vec![InMapperConfigChain {
                tag: None,
                chain: vec![
                    InMapperConfig::Listener("0.0.0.0:1080".to_string()),
                    InMapperConfig::Counter,
                    InMapperConfig::Socks5(PlainTextSet {
                        userpass: None,
                        more: None,
                    }),
                ],
            }],
            outbounds: vec![OutMapperConfigChain {
                tag: String::from("todo!()"),
                chain: vec![OutMapperConfig::Direct],
            }],
            ..Default::default()
        };
        let toml = toml::to_string(&sc).expect("valid toml");
        println!("{:#}", toml);

        let toml: StaticConfig = toml::from_str(&toml).expect("valid toml");
        println!("{:#?}", toml);
    }
}
