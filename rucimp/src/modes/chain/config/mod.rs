/*!
Defines the config format for chain, including static and dymatic ones.


主模块定义了静态链式配置 [`StaticConfig`] which can use lua or toml as config file format.

静态链是Map组成是运行前即知晓且依次按排列顺序执行的链,
因此可以用 Vec 表示

有限动态链的Map组成也用 [`StaticConfig`] 定义, 但其状态转移函数在
[`dynamic`] 模块中定义

完全动态链在 [`dynamic`] 模块中定义
 */

#[cfg(any(feature = "lua", feature = "lua54"))]
pub mod lua;

pub mod dynamic;

use std::{collections::HashMap, sync::Arc, time::Duration};

// #[cfg(feature = "s2n-quic")]
// use crate::map::quic;

use bytes::BytesMut;
use ruci::{
    map::{
        counter::Counter,
        echo::Echo,
        fold::{DMIterBox, DynVecIterWrapper},
        network::{BlackHole, Direct},
        *,
    },
    net::{self, dns, http::CommonConfig},
};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{
    map::{recorder, ws},
    utils::{init_tls_server_pem_option, FileSource},
};

#[cfg(feature = "lwip")]
use crate::map::tcp_ip_stack_lwip;

#[cfg(feature = "steganography")]
use crate::map::steganography::spe1;

#[cfg(all(feature = "sockopt", target_os = "linux"))]
use crate::map::tproxy::{self, TcpResolver};

#[cfg(feature = "route")]
use crate::route::{config::RuleSetConfig, RuleSet};

/// 静态配置中有初始化后即确定的 Map 数量
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct StaticConfig {
    pub inbounds: Vec<InMapConfigChain>,
    pub outbounds: Vec<OutMapConfigChain>,

    pub tag_route: Option<Vec<(String, String)>>,
    pub fallback_route: Option<Vec<(String, String)>>,

    #[cfg(feature = "route")]
    pub rule_route: Option<Vec<RuleSetConfig>>,
}

impl StaticConfig {
    /// convert config chain to map chain
    pub fn get_inbounds(&self, file_source: Arc<FileSource>) -> anyhow::Result<Vec<Vec<MapBox>>> {
        use anyhow::Context;
        use itertools::Itertools;

        let listens: Vec<_> = self
            .inbounds
            .iter()
            .map(|config_chain| {
                let chain: anyhow::Result<Vec<_>> = config_chain
                    .chain
                    .iter()
                    .map(|map_config| {
                        let config_with_fs = InMapConfigWithFileSource {
                            config: map_config.clone(),
                            file_source: file_source.clone(),
                        };

                        let map: anyhow::Result<MapBox> = config_with_fs
                            .try_into()
                            .context("config_with_fs.try_into failed");
                        map.map(|mut map| {
                            map.set_chain_tag(config_chain.tag.as_deref().unwrap_or(""));
                            map
                        })
                    })
                    .try_collect();

                chain.map(|mut chain| {
                    if let Some(last_m) = chain.last_mut() {
                        last_m.set_is_tail_of_chain(true);
                    } else {
                        warn!("the inbound chain has no maps, {:?}", config_chain.tag);
                    }
                    chain
                })
            })
            .try_collect()?;

        Ok(listens)
    }

    /// convert config chain to map chain
    pub fn get_outbounds(&self, file_source: Arc<FileSource>) -> anyhow::Result<Vec<Vec<MapBox>>> {
        use itertools::Itertools;
        self.outbounds
            .iter()
            .map(|config_chain| {
                let chain: anyhow::Result<Vec<_>> = config_chain
                    .chain
                    .iter()
                    .map(|map_config| {
                        let config_with_fs = OutMapConfigWithFileSource {
                            config: map_config.clone(),
                            file_source: file_source.clone(),
                        };

                        let map: anyhow::Result<MapBox> = config_with_fs.try_into();
                        map.map(|mut map| {
                            map.set_chain_tag(&config_chain.tag);
                            map
                        })
                    })
                    .try_collect();

                chain.map(|mut chain| {
                    if let Some(last_m) = chain.last_mut() {
                        last_m.set_is_tail_of_chain(true);
                    } else {
                        warn!("the outbound chain has no maps, {:?}", config_chain.tag);
                    }
                    chain
                })
            })
            .try_collect()
    }

    /// (out_tag, outbound)
    pub fn get_default_and_outbounds_map(
        &self,
        file_source: Arc<FileSource>,
    ) -> anyhow::Result<(DMIterBox, HashMap<String, DMIterBox>)> {
        let obs = self.get_outbounds(file_source)?;

        let mut first_o: Option<DMIterBox> = None;

        let o_map = obs
            .into_iter()
            .map(|outbound| {
                let tag = outbound
                    .first()
                    .expect("outbound should has at least one map ")
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
        Ok((first_o.expect("has an outbound"), o_map))
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
    pub fn get_rule_route(
        &self,
        file_source: Arc<crate::utils::FileSource>,
    ) -> Option<Vec<RuleSet>> {
        let mut result = self.rule_route.clone().map(|rr| {
            let v: Vec<RuleSet> = rr.into_iter().map(|r| r.to_rule_set()).collect();
            v
        });
        #[cfg(feature = "geoip")]
        {
            if let Some(mut rs_v) = result {
                use crate::route::maxmind;

                let r = maxmind::open_mmdb("Country.mmdb", file_source.as_ref());
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
pub struct InMapConfigChain {
    pub tag: Option<String>,
    pub chain: Vec<InMapConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct OutMapConfigChain {
    pub tag: String, //每个 out chain 都必须有一个 tag
    pub chain: Vec<OutMapConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DirectConfig {
    /// 此项是用于 建立 Direct 之后, Chain 中还有 后续的 Map, 且还需要读取target_addr时，使用
    /// 默认 Direct 将把 target_addr 消耗掉
    pub leak_target_addr: Option<bool>,
    pub dns_client: Option<dns::ClientConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct BindDialerConfig {
    pub bind_addr: Option<String>,
    pub dial_addr: Option<String>,

    pub dns_client: Option<dns::ClientConfig>,

    #[cfg(feature = "tun")]
    pub in_auto_route: Option<ruci::net::tun::route::InAutoRouteParams>,

    #[cfg(feature = "tun")]
    pub out_auto_route: Option<ruci::net::tun::route::OutAutoRouteParams>,

    pub ext: Option<Ext>,
}
impl TryFrom<Box<BindDialerConfig>> for MapBox {
    type Error = anyhow::Error;

    fn try_from(value: Box<BindDialerConfig>) -> Result<Self, Self::Error> {
        use anyhow::Context;

        let opt_bind_a = match value.bind_addr {
            Some(a) => {
                Some(net::Addr::from_name_network_addr_url(&a).context("network_ip_addr invalid")?)
            }
            None => None,
        };

        let opt_dial_a = match value.dial_addr {
            Some(a) => {
                Some(net::Addr::from_name_network_addr_url(&a).context("network_ip_addr invalid")?)
            }
            None => None,
        };

        let mut d = ruci::map::network::BindDialer::new();

        d.dial_addr = opt_dial_a;
        d.bind_addr = opt_bind_a;
        #[cfg(feature = "tun")]
        {
            d.in_auto_route = value.in_auto_route;
            d.out_auto_route = value.out_auto_route;
        }
        d.ext_fields = value.ext.as_ref().map(|e| e.to_ext_fields());

        d.opt_dns_client = value
            .dns_client
            .as_ref()
            .map(|dc| Arc::new(dns::AsyncClient::new(dc.clone())));

        Ok(Box::new(d))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct StdioConfig {
    pub write_mode: Option<ruci::map::stdio::WriteMode>,
    pub ext: Option<Ext>,
}

impl TryFrom<StdioConfig> for MapBox {
    type Error = anyhow::Error;

    fn try_from(value: StdioConfig) -> Result<Self, Self::Error> {
        let mut s = ruci::map::stdio::Stdio::default();

        if let Some(ext) = &value.ext {
            let ext_f = ext.to_ext_fields();

            s.set_ext_fields(Some(ext_f));
        }

        if let Some(m) = value.write_mode {
            s.write_mode = m;
        }
        Ok(Box::new(s))
    }
}

use strum_macros::EnumIter;

#[derive(Debug, Serialize, Deserialize, Clone, EnumIter)]
pub enum InMapConfig {
    Echo,                              //单流消耗器
    Stdio(StdioConfig),                //单流发生器
    Fileio(FileConfig),                //单流发生器
    BindDialer(Box<BindDialerConfig>), //单流发生器 (Box: #[warn(clippy::large_enum_variant)])
    Listener {
        listen_addr: String,
        ext: Option<Ext>,
    }, //多流发生器

    #[cfg(feature = "sockopt")]
    TcpOptListener {
        listen_addr: String,
        sockopt: crate::net::so2::SockOpt,
        ext: Option<Ext>,
    },

    #[cfg(all(feature = "sockopt", target_os = "linux"))]
    TproxyUdpListener {
        listen_addr: String,
        sockopt: crate::net::so2::SockOpt,
        ext: Option<Ext>,
    },

    #[cfg(all(feature = "sockopt", target_os = "linux"))]
    TproxyTcpResolver(tproxy::Options),

    Adder(i8),
    Counter,
    Recorder(recorder::Config),
    TLS(ruci_tls::server::TlsServerOptions),

    #[cfg(any(feature = "use-native-tls", feature = "native-tls-vendored"))]
    NativeTLS(ruci_tls::server::TlsServerOptions),
    H2 {
        is_grpc: Option<bool>,
        http_config: Option<CommonConfig>,
    },

    Http(PlainTextPassSet),
    Socks5(PlainTextPassSet),
    Socks5Http(PlainTextPassSet),
    Trojan(trojan::server::Config),
    HttpFilter(Option<CommonConfig>),
    WebSocket {
        http_config: Option<CommonConfig>,
    },
    // #[cfg(any(feature = "quic", feature = "quinn"))]
    #[cfg(feature = "quinn")]
    Quic(crate::map::quic_common::ServerConfig),

    /// tcp/ip stack
    #[cfg(feature = "smoltcp")]
    Stack,

    #[cfg(feature = "lwip")]
    StackLwip,

    #[cfg(feature = "steganography")]
    SPE1 {
        qa: Option<Vec<(String, String)>>,
    },

    /// Lua 自定义协议
    #[cfg(any(feature = "lua", feature = "lua54"))]
    Lua {
        file_name: String,          //如不给出，默认为直接使用该 lua 配置文件，但不建议
        handshake_function: String, // 用于 handshake 的 函数名
    },

    MITM(ruci_tls::server::TlsServerOptions),
}

#[derive(Debug, Serialize, Deserialize, Clone, EnumIter)]
pub enum OutMapConfig {
    Blackhole,                         //单流消耗器
    Direct(DirectConfig),              //单流发生器
    Stdio(StdioConfig),                //单流发生器
    Fileio(FileConfig),                //单流发生器
    BindDialer(Box<BindDialerConfig>), //单流发生器
    Adder(i8),
    Counter,
    Recorder(recorder::Config),
    TLS(ruci_tls::client::TlsClientOptions),

    #[cfg(feature = "sockopt")]
    OptDirect {
        sockopt: crate::net::so2::SockOpt,
        more_num_of_files: Option<bool>,
        dns_client: Option<dns::ClientConfig>,
    },

    #[cfg(feature = "sockopt")]
    OptDialer(crate::map::opt_net::OptDialerOption),

    #[cfg(any(feature = "use-native-tls", feature = "native-tls-vendored"))]
    NativeTLS(ruci_tls::client::TlsClientOptions),

    Http,
    Socks5(Socks5Out),
    Trojan(String),
    WebSocket(CommonConfig),
    H2Single {
        is_grpc: Option<bool>,

        http_config: Option<CommonConfig>,
    },
    H2Mux {
        is_grpc: Option<bool>,

        http_config: Option<CommonConfig>,
    },
    // #[cfg(any(feature = "quic", feature = "quinn"))]
    #[cfg(feature = "quinn")]
    Quic(crate::map::quic_common::ClientConfig),

    #[cfg(feature = "steganography")]
    SPE1 {
        qa: Option<Vec<(String, String)>>,
    },

    /// Lua 自定义协议
    #[cfg(any(feature = "lua", feature = "lua54"))]
    Lua {
        file_name: String,          //如不给出，默认为直接使用该 lua 配置文件，但不建议
        handshake_function: String, // 用于 handshake 的 函数名
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Ext {
    pub fixed_target_addr: Option<String>,

    pub pre_defined_early_data: Option<String>,
}
impl Ext {
    fn to_ext_fields(&self) -> MapExtFields {
        let mut ext_f = MapExtFields::default();
        if let Some(ta) = self.fixed_target_addr.as_ref() {
            ext_f.fixed_target_addr = net::Addr::from_network_addr_url(ta).ok();
        }
        if let Some(s) = self.pre_defined_early_data.as_ref() {
            ext_f.pre_defined_early_data = Some(BytesMut::from(s.as_bytes()));
        }
        ext_f
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct FileConfig {
    pub i: String,
    pub o: String,

    pub sleep_interval: Option<u64>,
    pub bytes_per_turn: Option<usize>,

    pub ext: Option<Ext>,
}

/// 明文密码配置
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PlainTextPassSet {
    pub userpass: Option<String>,
    pub more: Option<Vec<String>>,
    pub upgrade_to_h2: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Socks5Out {
    pub userpass: Option<String>,
    pub early_data: Option<bool>,

    pub ext: Option<Ext>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TrojanPassSet {
    pub password: Option<String>,
    pub more: Option<Vec<String>>,
}

pub struct InMapConfigWithFileSource {
    pub config: InMapConfig,
    pub file_source: Arc<FileSource>,
}

impl TryFrom<InMapConfig> for MapBox {
    type Error = anyhow::Error;

    fn try_from(config: InMapConfig) -> Result<Self, Self::Error> {
        let ic = InMapConfigWithFileSource {
            config,
            file_source: Arc::new(FileSource::StdReadFile),
        };
        ic.try_into()
    }
}

impl TryFrom<InMapConfigWithFileSource> for MapBox {
    type Error = anyhow::Error;

    fn try_from(value: InMapConfigWithFileSource) -> Result<Self, Self::Error> {
        let file_source = value.file_source;

        // let read_file_fn: Box<dyn Send + Fn(PathBuf) -> std::io::Result<String>> = {
        //     let fc = file_source.clone();

        //     let f = move |s: PathBuf| match fc.as_ref() {
        //         Some(fs) => fs
        //             .get_file_content(&s.to_string_lossy())
        //             .map(|(v, _)| String::from_utf8_lossy(v.as_slice()).to_string())
        //             .map_err(|e| {
        //                 tracing::debug!(
        //                     "get file content failed, file: {}, error: {}",
        //                     s.to_string_lossy(),
        //                     e
        //                 );
        //                 std::io::Error::other(e)
        //             }),
        //         None => {
        //             let r = std::fs::read_to_string(s);

        //             if r.is_err() {
        //                 tracing::debug!("std::fs::read_to_string failed");
        //             }
        //             r
        //         }
        //     };

        //     Box::new(f)
        // };

        match value.config {
            InMapConfig::Echo => Ok(Echo::boxed()),
            InMapConfig::Stdio(sc) => sc.try_into(),
            InMapConfig::Fileio(f) => {
                let s = ruci::map::fileio::FileIO {
                    i_name: f.i,
                    o_name: f.o,
                    sleep_interval: f.sleep_interval.map(Duration::from_millis),
                    bytes_per_turn: f.bytes_per_turn,
                    ext_fields: f.ext.map(|e| e.to_ext_fields()),
                };
                Ok(Box::new(s))
            }
            InMapConfig::BindDialer(dc) => dc.try_into(),
            InMapConfig::Listener { listen_addr, ext } => {
                let a =
                    net::Addr::from_network_addr_url(&listen_addr).expect("network_addr is valid");
                let g = ruci::map::network::Listener {
                    listen_addr: a,
                    ext_fields: ext.as_ref().map(|e| e.to_ext_fields()),
                };

                Ok(Box::new(g))
            }
            InMapConfig::Adder(i) => Ok(i.into()),
            InMapConfig::Counter => Ok(Counter::boxed()),
            InMapConfig::Recorder(c) => Ok(c.into()),

            InMapConfig::TLS(sc) => {
                let sc = crate::utils::init_tls_server_pem_option(&sc, file_source.as_ref())?;

                Ok(sc.into())
            }

            #[cfg(any(feature = "use-native-tls", feature = "native-tls-vendored"))]
            InMapConfig::NativeTLS(c) => Ok(Box::new(
                crate::map::native_tls::Server::from(&c, file_source.as_ref())
                    .expect("native_tls server config valid"),
            )),

            InMapConfig::Http(c) => {
                let sc = http_proxy::ServerConfig {
                    user_whitespace_pass: c.userpass,
                    user_passes: c.more.as_ref().map(|up_v| {
                        up_v.iter()
                            .map(|up| user_trait::PlainText::from(up.as_str()))
                            .collect::<Vec<_>>()
                    }),
                    ..Default::default()
                };

                Ok(sc.into())
            }
            InMapConfig::Socks5(c) => {
                let sc = socks5::server::Config {
                    support_udp: true, //默认打开udp 支持
                    user_whitespace_pass: c.userpass,
                    user_passes: c.more.as_ref().map(|up_v| {
                        up_v.iter()
                            .map(|up| user_trait::PlainText::from(up.as_str()))
                            .collect::<Vec<_>>()
                    }),
                };

                Ok(sc.into())
            }
            InMapConfig::Socks5Http(c) => {
                let sc = socks5http::Config {
                    user_whitespace_pass: c.userpass,
                    user_passes: c.more.as_ref().map(|up_v| {
                        up_v.iter()
                            .map(|up| user_trait::PlainText::from(up.as_str()))
                            .collect::<Vec<_>>()
                    }),
                };

                Ok(sc.into())
            }
            InMapConfig::Trojan(sc) => Ok(sc.into()),
            InMapConfig::WebSocket {
                http_config: config,
            } => Ok(Box::new(crate::map::ws::server::Server {
                config,
                ..Default::default()
            })),
            InMapConfig::HttpFilter(c) => Ok(Box::new(ruci::map::http_filter::Server {
                config: c,
                ..Default::default()
            })),
            InMapConfig::H2 {
                http_config: config,
                is_grpc,
            } => Ok(Box::new(crate::map::h2::server::Server::new(
                is_grpc, config,
            ))),
            // #[cfg(feature = "quic")]
            // InMapConfig::Quic(c) => Ok(Box::new(quic::server::Server::new(c))),
            #[cfg(feature = "quinn")]
            InMapConfig::Quic(c) => Ok(Box::new(crate::map::quinn::server::Server::new(
                c,
                &file_source,
            )?)),

            #[cfg(feature = "sockopt")]
            InMapConfig::TcpOptListener {
                listen_addr,
                sockopt,
                ext,
            } => Ok(Box::new(crate::map::opt_net::TcpOptListener {
                listen_addr: net::Addr::from_network_addr_url(&listen_addr)?,
                sopt: sockopt,
                ext_fields: ext.as_ref().map(|e| e.to_ext_fields()),
            })),

            #[cfg(all(feature = "sockopt", target_os = "linux"))]
            InMapConfig::TproxyTcpResolver(opts) => Ok(Box::new(TcpResolver::new(opts)?)),

            #[cfg(all(feature = "sockopt", target_os = "linux"))]
            InMapConfig::TproxyUdpListener {
                listen_addr,
                sockopt,
                ext,
            } => Ok(Box::new(crate::map::tproxy::UDPListener {
                listen_addr: net::Addr::from_network_addr_url(&listen_addr)?,
                sopt: sockopt,
                ext_fields: ext.as_ref().map(|e| e.to_ext_fields()),
            })),
            #[cfg(feature = "smoltcp")]
            InMapConfig::Stack => Ok(Box::<crate::map::tcp_ip_stack_smoltcp::Stack>::default()),

            #[cfg(feature = "steganography")]
            InMapConfig::SPE1 { qa } => Ok(Box::new(spe1::ClientOrServer {
                qa: Arc::new(match qa {
                    Some(qa) => spe1::QaData::from(qa.to_vec()),
                    None => spe1::QaData::new_simple(),
                }),
                is_server: true,
                ext_fields: Some(MapExtFields::default()),
            })),
            #[cfg(any(feature = "lua", feature = "lua54"))]
            InMapConfig::Lua {
                file_name,
                handshake_function,
            } => {
                let lua_text = file_source.read_to_string(&file_name)?;

                Ok(Box::new(crate::map::lua::LuaMap {
                    lua_text,
                    handshake_f_key: handshake_function.to_string(),
                    ext_fields: Some(MapExtFields::default()),
                    file_source,
                }))
            }
            #[cfg(feature = "lwip")]
            InMapConfig::StackLwip => Ok(Box::new(tcp_ip_stack_lwip::Stack {
                ext_fields: Some(MapExtFields::default()),
            })),
            InMapConfig::MITM(c) => {
                let sc = init_tls_server_pem_option(&c, &file_source)?;

                Ok(Box::new(ruci_tls::mitm::MITM {
                    sc,
                    ext_fields: None,
                }))
            }
        }
    }
}
pub struct OutMapConfigWithFileSource {
    pub config: OutMapConfig,
    pub file_source: Arc<FileSource>,
}

impl TryFrom<OutMapConfig> for MapBox {
    type Error = anyhow::Error;

    fn try_from(config: OutMapConfig) -> Result<Self, Self::Error> {
        let ic = OutMapConfigWithFileSource {
            config,
            file_source: Arc::new(FileSource::StdReadFile),
        };
        ic.try_into()
    }
}

impl TryFrom<OutMapConfigWithFileSource> for MapBox {
    type Error = anyhow::Error;

    #[allow(unused)]
    fn try_from(value: OutMapConfigWithFileSource) -> Result<Self, Self::Error> {
        use anyhow::Context;

        let file_source = value.file_source;

        // let read_file_fn: Box<dyn Send + Fn(PathBuf) -> std::io::Result<String>> = {
        //     let fc = file_source.clone();

        //     let f = move |s: PathBuf| match fc.as_ref() {
        //         Some(fs) => fs
        //             .get_file_content(&s.to_string_lossy())
        //             .map(|(v, _)| String::from_utf8_lossy(v.as_slice()).to_string())
        //             .map_err(std::io::Error::other),
        //         None => std::fs::read_to_string(s),
        //     };

        //     Box::new(f)
        // };

        match value.config {
            OutMapConfig::Stdio(sc) => sc.try_into(),
            OutMapConfig::Fileio(f) => {
                let s = ruci::map::fileio::FileIO {
                    i_name: f.i,
                    o_name: f.o,
                    sleep_interval: f.sleep_interval.map(Duration::from_millis),
                    bytes_per_turn: f.bytes_per_turn,
                    ext_fields: f.ext.map(|e| e.to_ext_fields()),
                };
                Ok(Box::new(s))
            }
            OutMapConfig::Blackhole => Ok(BlackHole::boxed()),

            OutMapConfig::Direct(dc) => {
                let mut m = Box::<Direct>::default();
                if let Some(dc) = &dc.dns_client {
                    m.opt_dns_client = Some(Arc::new(dns::AsyncClient::new(dc.clone())));
                }
                m.leak_target_addr = dc.leak_target_addr.unwrap_or_default();
                Ok(m)
            }
            OutMapConfig::BindDialer(dc) => dc.try_into(),
            OutMapConfig::Adder(i) => Ok(i.into()),
            OutMapConfig::Counter => Ok(Box::<counter::Counter>::default()),
            OutMapConfig::Recorder(c) => Ok(c.into()),

            OutMapConfig::TLS(c) => Ok(c.into()),

            #[cfg(any(feature = "use-native-tls", feature = "native-tls-vendored"))]
            OutMapConfig::NativeTLS(c) => Ok(Box::new(crate::map::native_tls::Client {
                config: c,
                ext_fields: Some(MapExtFields::default()),
            })),
            OutMapConfig::Http => Ok(http_proxy::Client::boxed()),
            OutMapConfig::Socks5(c) => {
                let u = c.userpass.unwrap_or_default();
                let mut a = socks5::client::Client {
                    up: if u.is_empty() {
                        None
                    } else {
                        Some(user_trait::PlainText::from(u.as_str()))
                    },
                    use_earlydata: c.early_data.unwrap_or_default(),
                    ..Default::default()
                };
                if let Some(ext) = &c.ext {
                    a.set_ext_fields(Some(ext.to_ext_fields()))
                }
                Ok(Box::new(a))
            }
            OutMapConfig::Trojan(pass) => {
                let a = trojan::client::Client::new(&pass);
                Ok(Box::new(a))
            }
            OutMapConfig::WebSocket(c) => {
                let client = ws::client::Client::new(c);

                Ok(Box::new(client))
            }
            OutMapConfig::H2Single {
                http_config: config,
                is_grpc,
            } => Ok(Box::new(crate::map::h2::client::SingleClient::new(
                is_grpc.unwrap_or_default(),
                config,
            ))),
            OutMapConfig::H2Mux {
                http_config: config,
                is_grpc,
            } => {
                let m = crate::map::h2::client::MuxClient::new(is_grpc.unwrap_or_default(), config);

                Ok(Box::new(m))
            }
            // #[cfg(feature = "quic")]
            // OutMapConfig::Quic(c) => Ok(Box::new(
            //     quic::client::Client::new(c).expect("legal quic client config"),
            // )),
            #[cfg(feature = "quinn")]
            OutMapConfig::Quic(c) => Ok(Box::new(
                crate::map::quinn::client::Client::new(c, &file_source)
                    .context("load quic client config failed")?,
            )),

            #[cfg(feature = "sockopt")]
            OutMapConfig::OptDirect {
                sockopt,
                more_num_of_files,
                dns_client,
            } => Ok(Box::new(crate::map::opt_net::OptDirect::new(
                sockopt,
                more_num_of_files,
                dns_client
                    .as_ref()
                    .map(|c| Arc::new(dns::AsyncClient::new(c.clone()))),
            )?)),
            #[cfg(feature = "sockopt")]
            OutMapConfig::OptDialer(sopt) => {
                Ok(Box::new(crate::map::opt_net::OptDialer::new(sopt)?))
            }

            #[cfg(feature = "steganography")]
            OutMapConfig::SPE1 { qa } => Ok(Box::new(spe1::ClientOrServer {
                qa: Arc::new(match qa {
                    Some(qa) => spe1::QaData::from(qa.to_vec()),
                    None => spe1::QaData::new_simple(),
                }),
                is_server: false,
                ext_fields: Some(MapExtFields::default()),
            })),
            #[cfg(any(feature = "lua", feature = "lua54"))]
            OutMapConfig::Lua {
                file_name,
                handshake_function,
            } => {
                let r = crate::utils::try_get_file_content("", Some(&file_name));
                match r {
                    Ok(lua_bytes) => Ok(Box::new(crate::map::lua::LuaMap {
                        lua_text: String::from_utf8_lossy(lua_bytes.as_slice()).to_string(),
                        handshake_f_key: handshake_function.to_string(),
                        ext_fields: Some(MapExtFields::default()),
                        file_source: file_source.clone(),
                    })),
                    Err(_) => todo!(),
                }
            }
        }
    }
}
