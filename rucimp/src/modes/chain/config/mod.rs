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

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

#[cfg(feature = "s2n-quic")]
use crate::map::quic;

use bytes::BytesMut;
use recorder::RecorderMap;
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
    utils::FileSource,
};

#[cfg(feature = "lwip")]
use crate::map::tcp_ip_stack_lwip;

#[cfg(feature = "steganography")]
use crate::map::spe1;

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
    pub fn get_inbounds(&self, file_source: Option<&FileSource>) -> Vec<Vec<MapBox>> {
        let listens: Vec<_> = self
            .inbounds
            .iter()
            .map(|config_chain| {
                let mut chain = config_chain
                    .chain
                    .iter()
                    .map(|map_config| {
                        let mut map = map_config.to_map_box(file_source);
                        map.set_chain_tag(config_chain.tag.as_deref().unwrap_or(""));
                        map
                    })
                    .collect::<Vec<_>>();

                if let Some(last_m) = chain.last_mut() {
                    last_m.set_is_tail_of_chain(true);
                } else {
                    warn!("the inbound chain has no maps, {:?}", config_chain.tag);
                }

                chain
            })
            .collect();

        listens
    }

    /// convert config chain to map chain
    pub fn get_outbounds(&self, file_source: Option<&FileSource>) -> Vec<Vec<MapBox>> {
        self.outbounds
            .iter()
            .map(|config_chain| {
                let mut chain = config_chain
                    .chain
                    .iter()
                    .map(|map_config| {
                        let mut map = map_config.to_map_box(file_source);
                        map.set_chain_tag(&config_chain.tag);
                        map
                    })
                    .collect::<Vec<_>>();

                if let Some(last_m) = chain.last_mut() {
                    last_m.set_is_tail_of_chain(true);
                } else {
                    warn!("the outbound chain has no maps, {:?}", config_chain.tag);
                }

                chain
            })
            .collect::<Vec<_>>()
    }

    /// (out_tag, outbound)
    pub fn get_default_and_outbounds_map(
        &self,
        file_source: Option<&FileSource>,
    ) -> (DMIterBox, HashMap<String, DMIterBox>) {
        let obs = self.get_outbounds(file_source);

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
    pub fn get_rule_route(&self, file_source: &crate::utils::FileSource) -> Option<Vec<RuleSet>> {
        let mut result = self.rule_route.clone().map(|rr| {
            let x: Vec<RuleSet> = rr.into_iter().map(|r| r.to_rule_set()).collect();
            x
        });
        #[cfg(feature = "geoip")]
        {
            if let Some(mut rs_v) = result {
                use crate::route::maxmind;

                let r = maxmind::open_mmdb("Country.mmdb", file_source);
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
    tag: Option<String>,
    chain: Vec<InMapConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct OutMapConfigChain {
    tag: String, //每个 out chain 都必须有一个 tag
    chain: Vec<OutMapConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DirectConfig {
    dns_client: Option<dns::ClientConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct BindDialerConfig {
    bind_addr: Option<String>,
    dial_addr: Option<String>,

    dns_client: Option<dns::ClientConfig>,

    #[cfg(feature = "tun")]
    in_auto_route: Option<ruci::net::tun::route::InAutoRouteParams>,

    #[cfg(feature = "tun")]
    out_auto_route: Option<ruci::net::tun::route::OutAutoRouteParams>,

    ext: Option<Ext>,
}
impl ToMapBox for BindDialerConfig {
    fn to_map_box(&self) -> MapBox {
        let opt_bind_a = self
            .bind_addr
            .clone()
            .map(|a| net::Addr::from_name_network_addr_url(&a).expect("network_ip_addr is valid"));

        let opt_dial_a = self
            .dial_addr
            .clone()
            .map(|a| net::Addr::from_name_network_addr_url(&a).expect("network_ip_addr is valid"));
        let mut d = ruci::map::network::BindDialer::new();

        d.dial_addr = opt_dial_a;
        d.bind_addr = opt_bind_a;
        #[cfg(feature = "tun")]
        {
            d.in_auto_route = self.in_auto_route.clone();
            d.out_auto_route = self.out_auto_route.clone();
        }
        d.ext_fields = self.ext.as_ref().map(|e| e.to_ext_fields());

        d.opt_dns_client = self
            .dns_client
            .as_ref()
            .map(|dc| Arc::new(dns::AsyncClient::new(dc.clone())));

        Box::new(d)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct StdioConfig {
    pub write_mode: Option<ruci::map::stdio::WriteMode>,
    pub ext: Option<Ext>,
}

impl ToMapBox for StdioConfig {
    fn to_map_box(&self) -> MapBox {
        let mut s = ruci::map::stdio::Stdio::default();

        if let Some(ext) = &self.ext {
            let ext_f = ext.to_ext_fields();

            s.set_ext_fields(Some(ext_f));
        }

        if let Some(m) = self.write_mode {
            s.write_mode = m;
        }
        Box::new(s)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    TLS(TlsIn),

    #[cfg(any(feature = "use-native-tls", feature = "native-tls-vendored"))]
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
    #[cfg(any(feature = "quic", feature = "quinn"))]
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
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum OutMapConfig {
    Blackhole,                         //单流消耗器
    Direct(DirectConfig),              //单流发生器
    Stdio(StdioConfig),                //单流发生器
    Fileio(FileConfig),                //单流发生器
    BindDialer(Box<BindDialerConfig>), //单流发生器
    Adder(i8),
    Counter,
    Recorder(recorder::Config),
    TLS(TlsOut),

    #[cfg(feature = "sockopt")]
    OptDirect {
        sockopt: crate::net::so2::SockOpt,
        more_num_of_files: Option<bool>,
        dns_client: Option<dns::ClientConfig>,
    },

    #[cfg(feature = "sockopt")]
    OptDialer(crate::map::opt_net::OptDialerOption),

    #[cfg(any(feature = "use-native-tls", feature = "native-tls-vendored"))]
    NativeTLS(TlsOut),

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
    #[cfg(any(feature = "quic", feature = "quinn"))]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileConfig {
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

pub trait AdvancedToMapBox {
    fn to_map_box(&self, file_source: Option<&FileSource>) -> MapBox;
}

impl AdvancedToMapBox for InMapConfig {
    fn to_map_box(&self, file_source: Option<&FileSource>) -> ruci::map::MapBox {
        match self {
            InMapConfig::Echo => Box::<Echo>::default(),
            InMapConfig::Stdio(sc) => sc.to_map_box(),
            InMapConfig::Fileio(f) => {
                let s = ruci::map::fileio::FileIO {
                    i_name: f.i.clone(),
                    o_name: f.o.clone(),
                    sleep_interval: f.sleep_interval.map(Duration::from_millis),
                    bytes_per_turn: f.bytes_per_turn,
                    ext_fields: f.ext.clone().map(|e| e.to_ext_fields()),
                };
                Box::new(s)
            }
            InMapConfig::BindDialer(dc) => dc.to_map_box(),
            InMapConfig::Listener { listen_addr, ext } => {
                let a =
                    net::Addr::from_network_addr_url(listen_addr).expect("network_addr is valid");
                let g = ruci::map::network::Listener {
                    listen_addr: a,
                    ext_fields: ext.as_ref().map(|e| e.to_ext_fields()),
                };

                Box::new(g)
            }
            InMapConfig::Adder(i) => i.to_map_box(),
            InMapConfig::Counter => Box::<Counter>::default(),
            InMapConfig::Recorder(c) => Box::new(RecorderMap::new(c.clone())),

            InMapConfig::TLS(c) => tls::server::ServerOptions {
                cert: PathBuf::from(c.cert.clone()),
                key: PathBuf::from(c.key.clone()),
                alpn: c.alpn.clone(),
            }
            .to_map_box(),

            #[cfg(any(feature = "use-native-tls", feature = "native-tls-vendored"))]
            InMapConfig::NativeTLS(c) => Box::new(
                crate::map::native_tls::ServerOptions {
                    cert_f_path: c.cert.clone(),
                    key_f_path: c.key.clone(),
                }
                .get_server()
                .unwrap(),
            ),

            InMapConfig::Http(c) => {
                let so = http_proxy::ServerConfig {
                    user_whitespace_pass: c.userpass.clone(),
                    user_passes: c.more.as_ref().map(|up_v| {
                        up_v.iter()
                            .map(|up| ruci::user::PlainText::from(up.to_string()))
                            .collect::<Vec<_>>()
                    }),
                    ..Default::default()
                };

                so.to_map_box()
            }
            InMapConfig::Socks5(c) => {
                let so = socks5::server::Config {
                    support_udp: true, //默认打开udp 支持
                    user_whitespace_pass: c.userpass.clone(),
                    user_passes: c.more.as_ref().map(|up_v| {
                        up_v.iter()
                            .map(|up| ruci::user::PlainText::from(up.to_string()))
                            .collect::<Vec<_>>()
                    }),
                };

                so.to_map_box()
            }
            InMapConfig::Socks5Http(c) => {
                let so = socks5http::Config {
                    user_whitespace_pass: c.userpass.clone(),
                    user_passes: c.more.as_ref().map(|up_v| {
                        up_v.iter()
                            .map(|up| ruci::user::PlainText::from(up.to_string()))
                            .collect::<Vec<_>>()
                    }),
                };

                so.to_map_box()
            }
            InMapConfig::Trojan(c) => {
                let so = trojan::server::Config {
                    pass: c.password.clone(),
                    passes: c.more.as_ref().map(|up_v| up_v.to_vec()),
                };

                so.to_map_box()
            }
            InMapConfig::WebSocket {
                http_config: config,
            } => Box::new(crate::map::ws::server::Server {
                config: config.clone(),
                ..Default::default()
            }),
            InMapConfig::HttpFilter(c) => Box::new(ruci::map::http_filter::Server {
                config: c.clone(),
                ..Default::default()
            }),
            InMapConfig::H2 {
                http_config: config,
                is_grpc,
            } => Box::new(crate::map::h2::server::Server::new(
                *is_grpc,
                config.clone(),
            )),
            #[cfg(feature = "quic")]
            InMapConfig::Quic(c) => Box::new(quic::server::Server::new(c.clone())),

            #[cfg(feature = "quinn")]
            InMapConfig::Quic(c) => Box::new(crate::map::quinn::server::Server::new(c.clone())),

            #[cfg(feature = "sockopt")]
            InMapConfig::TcpOptListener {
                listen_addr,
                sockopt,
                ext,
            } => Box::new(crate::map::opt_net::TcpOptListener {
                listen_addr: net::Addr::from_network_addr_url(listen_addr).expect("listen_addr ok"),
                sopt: sockopt.clone(),
                ext_fields: ext.as_ref().map(|e| e.to_ext_fields()),
            }),

            #[cfg(all(feature = "sockopt", target_os = "linux"))]
            InMapConfig::TproxyTcpResolver(opts) => {
                Box::new(TcpResolver::new(opts.clone()).expect("ok"))
            }

            #[cfg(all(feature = "sockopt", target_os = "linux"))]
            InMapConfig::TproxyUdpListener {
                listen_addr,
                sockopt,
                ext,
            } => Box::new(crate::map::tproxy::UDPListener {
                listen_addr: net::Addr::from_network_addr_url(listen_addr).expect("listen_addr ok"),
                sopt: sockopt.clone(),
                ext_fields: ext.as_ref().map(|e| e.to_ext_fields()),
            }),
            #[cfg(feature = "smoltcp")]
            InMapConfig::Stack => Box::<crate::map::tcp_ip_stack_smoltcp::Stack>::default(),

            #[cfg(feature = "steganography")]
            InMapConfig::SPE1 { qa } => Box::new(spe1::ClientOrServer {
                qa: Arc::new(match qa {
                    Some(qa) => spe1::QaData::from(qa.to_vec()),
                    None => spe1::QaData::new_simple(),
                }),
                is_server: true,
                ext_fields: Some(MapExtFields::default()),
            }),
            #[cfg(any(feature = "lua", feature = "lua54"))]
            InMapConfig::Lua {
                file_name,
                handshake_function,
            } => {
                let r = crate::utils::try_get_file_content("", Some(file_name));
                match r {
                    Ok(lua_bytes) => Box::new(crate::map::lua::LuaMap {
                        lua_text: String::from_utf8_lossy(lua_bytes.as_slice()).to_string(),
                        handshake_f_key: handshake_function.to_string(),
                        ext_fields: Some(MapExtFields::default()),
                        file_source: file_source.cloned(),
                    }),
                    Err(e) => panic!("get lua file content err {e}"),
                }
            }
            #[cfg(feature = "lwip")]
            InMapConfig::StackLwip => Box::new(tcp_ip_stack_lwip::Stack {
                ext_fields: Some(MapExtFields::default()),
            }),
        }
    }
}
impl AdvancedToMapBox for OutMapConfig {
    fn to_map_box(&self, file_source: Option<&FileSource>) -> ruci::map::MapBox {
        match self {
            OutMapConfig::Stdio(sc) => sc.to_map_box(),
            OutMapConfig::Fileio(f) => {
                let s = ruci::map::fileio::FileIO {
                    i_name: f.i.clone(),
                    o_name: f.o.clone(),
                    sleep_interval: f.sleep_interval.map(Duration::from_millis),
                    bytes_per_turn: f.bytes_per_turn,
                    ext_fields: f.ext.clone().map(|e| e.to_ext_fields()),
                };
                Box::new(s)
            }
            OutMapConfig::Blackhole => Box::<BlackHole>::default(),

            OutMapConfig::Direct(dc) => {
                let mut m = Box::<Direct>::default();
                if let Some(dc) = &dc.dns_client {
                    m.opt_dns_client = Some(Arc::new(dns::AsyncClient::new(dc.clone())));
                }
                m
            }
            OutMapConfig::BindDialer(dc) => dc.to_map_box(),
            OutMapConfig::Adder(i) => i.to_map_box(),
            OutMapConfig::Counter => Box::<counter::Counter>::default(),
            OutMapConfig::Recorder(c) => Box::new(RecorderMap::new(c.clone())),

            OutMapConfig::TLS(c) => {
                let a = tls::client::Client::new(tls::client::ClientOptions {
                    domain: c.host.clone(),
                    is_insecure: c.insecure.unwrap_or_default(),
                    alpn: c.alpn.clone(),
                });
                Box::new(a)
            }

            #[cfg(any(feature = "use-native-tls", feature = "native-tls-vendored"))]
            OutMapConfig::NativeTLS(c) => Box::new(crate::map::native_tls::Client {
                domain: c.host.clone(),
                insecure: c.insecure.unwrap_or_default(),
                alpn: c.alpn.clone(),
                ext_fields: Some(MapExtFields::default()),
            }),
            OutMapConfig::Http => Box::new(http_proxy::Client::default()),
            OutMapConfig::Socks5(c) => {
                let u = c.userpass.clone().unwrap_or_default();
                let mut a = socks5::client::Client {
                    up: if u.is_empty() {
                        None
                    } else {
                        Some(ruci::user::PlainText::from(u))
                    },
                    use_earlydata: c.early_data.unwrap_or_default(),
                    ..Default::default()
                };
                if let Some(ext) = &c.ext {
                    a.set_ext_fields(Some(ext.to_ext_fields()))
                }
                Box::new(a)
            }
            OutMapConfig::Trojan(pass) => {
                let a = trojan::client::Client::new(pass);
                Box::new(a)
            }
            OutMapConfig::WebSocket(c) => {
                let client = ws::client::Client::new(c.clone());

                Box::new(client)
            }
            OutMapConfig::H2Single {
                http_config: config,
                is_grpc,
            } => Box::new(crate::map::h2::client::SingleClient::new(
                is_grpc.unwrap_or_default(),
                config.clone(),
            )),
            OutMapConfig::H2Mux {
                http_config: config,
                is_grpc,
            } => {
                let m = crate::map::h2::client::MuxClient::new(
                    is_grpc.unwrap_or_default(),
                    config.clone(),
                );

                Box::new(m)
            }
            #[cfg(feature = "quic")]
            OutMapConfig::Quic(c) => {
                Box::new(quic::client::Client::new(c.clone()).expect("legal quic client config"))
            }

            #[cfg(feature = "quinn")]
            OutMapConfig::Quic(c) => Box::new(
                crate::map::quinn::client::Client::new(c.clone())
                    .expect("legal quic client config"),
            ),

            #[cfg(feature = "sockopt")]
            OutMapConfig::OptDirect {
                sockopt,
                more_num_of_files,
                dns_client,
            } => Box::new(
                crate::map::opt_net::OptDirect::new(
                    sockopt.clone(),
                    *more_num_of_files,
                    dns_client
                        .as_ref()
                        .map(|c| Arc::new(dns::AsyncClient::new(c.clone()))),
                )
                .expect("ok"),
            ),
            #[cfg(feature = "sockopt")]
            OutMapConfig::OptDialer(sopt) => {
                Box::new(crate::map::opt_net::OptDialer::new(sopt.clone()).expect("ok"))
            }

            #[cfg(feature = "steganography")]
            OutMapConfig::SPE1 { qa } => Box::new(spe1::ClientOrServer {
                qa: Arc::new(match qa {
                    Some(qa) => spe1::QaData::from(qa.to_vec()),
                    None => spe1::QaData::new_simple(),
                }),
                is_server: false,
                ext_fields: Some(MapExtFields::default()),
            }),
            #[cfg(any(feature = "lua", feature = "lua54"))]
            OutMapConfig::Lua {
                file_name,
                handshake_function,
            } => {
                let r = crate::utils::try_get_file_content("", Some(file_name));
                match r {
                    Ok(lua_bytes) => Box::new(crate::map::lua::LuaMap {
                        lua_text: String::from_utf8_lossy(lua_bytes.as_slice()).to_string(),
                        handshake_f_key: handshake_function.to_string(),
                        ext_fields: Some(MapExtFields::default()),
                        file_source: file_source.cloned(),
                    }),
                    Err(_) => todo!(),
                }
            }
        }
    }
}

#[cfg(test)]
mod test {

    #[allow(unused)]
    use dns::ClientConfig;

    use super::*;
    #[test]
    #[cfg(feature = "toml")]
    fn serialize_toml() {
        let sa = std::net::SocketAddr::V4("114.114.114.114:53".parse().unwrap());
        let sc = StaticConfig {
            inbounds: vec![InMapConfigChain {
                tag: None,
                chain: vec![
                    InMapConfig::Listener {
                        listen_addr: "0.0.0.0:1080".to_string(),
                        ext: None,
                    },
                    InMapConfig::Counter,
                    InMapConfig::Socks5(PlainTextSet {
                        userpass: None,
                        more: None,
                    }),
                ],
            }],
            outbounds: vec![OutMapConfigChain {
                tag: String::from("todo!()"),
                chain: vec![
                    OutMapConfig::Direct(DirectConfig { dns_client: None }),
                    OutMapConfig::Direct(DirectConfig {
                        dns_client: Some(ClientConfig {
                            dns_server_list: vec![(sa, dns::TheProtocol::Udp)],
                            ip_strategy: Some(dns::TheLookupIpStrategy::Ipv4Only),
                            static_pairs: HashMap::new(),
                        }),
                    }),
                ],
            }],
            ..Default::default()
        };
        let toml = toml::to_string(&sc).expect("valid toml");
        println!("{:#}", toml);

        let toml: StaticConfig = toml::from_str(&toml).expect("valid toml");
        println!("{:#?}", toml);
    }
}
