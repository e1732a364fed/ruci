/*!
 * 链式配置
 */

#[cfg(feature = "lua")]
pub mod lua;

#[cfg(feature = "lua")]
pub mod dynamic;

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use ruci::{
    map::{acc::MIterBox, *},
    net,
};
use serde::{Deserialize, Serialize};

/// 静态配置中有初始化后即确定的listen/dial数量和行为
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct StaticConfig {
    pub inbounds: Vec<InMapperConfigChain>,
    pub outbounds: Vec<OutMapperConfigChain>,

    pub tag_route: Option<Vec<(String, String)>>,
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
                        let mut mapper = mapper_config.to_mapper();
                        mapper.set_chain_tag(config_chain.tag.as_ref().unwrap_or(&String::new()));
                        mapper
                    })
                    .collect::<Vec<_>>();

                chain
                    .last_mut()
                    .expect("has a inbound")
                    .set_is_tail_of_chain(true);
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
                        let mut mapper = mapper_config.to_mapper();
                        mapper.set_chain_tag(&config_chain.tag);
                        mapper
                    })
                    .collect::<Vec<_>>();

                chain
                    .last_mut()
                    .expect("has an outbound")
                    .set_is_tail_of_chain(true);
                chain
            })
            .collect::<Vec<_>>()
    }

    /// (out_tag, outbound)
    pub fn get_default_and_outbounds_map(&self) -> (MIterBox, HashMap<String, MIterBox>) {
        let obs = self.get_outbounds();

        let mut first_o: Option<MIterBox> = None;

        let omap = obs
            .into_iter()
            .map(|outbound| {
                let tag = outbound
                    .iter()
                    .next()
                    .expect("outbound has one mapper at least")
                    .get_chain_tag();

                let ts = tag.to_string();
                let outbound: Vec<_> = outbound.into_iter().map(|o| Arc::new(o)).collect();

                let outbound_iter: MIterBox = Box::new(outbound.into_iter());

                if let None = first_o {
                    first_o = Some(outbound_iter.clone());
                }

                (ts, outbound_iter)
            })
            .collect();
        (first_o.expect("has a outbound"), omap)
    }

    /// panic if the given tag isn't presented in outbounds
    pub fn get_tag_route(&self) -> Option<HashMap<String, String>> {
        self.tag_route.as_ref().map(|tr| {
            let route_tag_pairs = tr.clone();
            let route_tag_map = route_tag_pairs.into_iter().collect::<HashMap<_, _>>();

            route_tag_map
        })
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
    Stdio(String),
    Listener(String),
    Adder(i8),
    Counter,
    TLS(TlsIn),
    Http(UserPassField),
    Socks5(UserPassField),
    Socks5Http(UserPassField),
    Trojan(TrojanIn),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum OutMapperConfig {
    Direct,
    Blackhole,
    Stdio(String),
    Dialer(String),
    Adder(i8),
    Counter,
    TLS(TlsOut),
    Socks5(Socks5Out),
    Trojan(String),
}

// #[derive(Debug, Serialize, Deserialize, Clone)]
// pub enum Dialer {
//     TcpDialer(String),
//     UnixDialer(String),
// }

// #[derive(Debug, Serialize, Deserialize, Clone)]
// pub enum Listener {
//     TcpListener(String),
//     UnixListener(String),
// }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TlsIn {
    cert: String,
    key: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TlsOut {
    host: String,
    insecure: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserPassField {
    userpass: Option<String>,
    more: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Socks5Out {
    userpass: String,
    early_data: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrojanIn {
    password: Option<String>,
    more: Option<Vec<String>>,
}

impl ToMapper for InMapperConfig {
    fn to_mapper(&self) -> ruci::map::MapperBox {
        match self {
            InMapperConfig::Stdio(s) => {
                ruci::map::stdio::Stdio::from(s).expect("s is a  network addr str")
            }
            InMapperConfig::Listener(l_str) => {
                let a = net::Addr::from_ip_addr_str("tcp", l_str).expect("ip_addr is valid");
                let mut g = ruci::map::network::StreamGenerator::default();
                g.set_configured_target_addr(Some(a));
                Box::new(g)
            }
            InMapperConfig::Adder(i) => i.to_mapper(),
            InMapperConfig::Counter => Box::new(ruci::map::counter::Counter::default()),
            InMapperConfig::TLS(c) => tls::server::ServerOptions {
                addr: "todo!()".to_string(),
                cert: PathBuf::from(c.cert.clone()),
                key: PathBuf::from(c.key.clone()),
            }
            .to_mapper(),
            InMapperConfig::Http(c) => {
                let so = http::Config {
                    user_whitespace_pass: c.userpass.clone(),
                    user_passes: c.more.as_ref().map(|up_v| {
                        up_v.iter()
                            .map(|up| ruci::user::PlainText::from(up.to_string()))
                            .collect::<Vec<_>>()
                    }),
                    ..Default::default()
                };

                so.to_mapper()
            }
            InMapperConfig::Socks5(c) => {
                let so = socks5::server::Config {
                    user_whitespace_pass: c.userpass.clone(),
                    user_passes: c.more.as_ref().map(|up_v| {
                        up_v.iter()
                            .map(|up| ruci::user::PlainText::from(up.to_string()))
                            .collect::<Vec<_>>()
                    }),
                    ..Default::default()
                };

                so.to_mapper()
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

                so.to_mapper()
            }
            InMapperConfig::Trojan(c) => {
                let so = trojan::server::Config {
                    pass: c.password.clone(),
                    passes: c.more.as_ref().map(|up_v| up_v.to_vec()),
                };

                so.to_mapper()
            }
        }
    }
}

impl ToMapper for OutMapperConfig {
    fn to_mapper(&self) -> ruci::map::MapperBox {
        match self {
            OutMapperConfig::Stdio(s) => {
                ruci::map::stdio::Stdio::from(s).expect("s is a network addr str")
            }
            OutMapperConfig::Blackhole => Box::new(ruci::map::network::BlackHole::default()),

            OutMapperConfig::Direct => Box::new(ruci::map::network::Direct::default()),
            OutMapperConfig::Dialer(td_str) => {
                let a = net::Addr::from_network_addr_str(td_str).expect("network_ip_addr is valid");
                let mut d = ruci::map::network::Dialer::default();
                d.set_configured_target_addr(Some(a));
                Box::new(d)
            }
            OutMapperConfig::Adder(i) => i.to_mapper(),
            OutMapperConfig::Counter => Box::new(ruci::map::counter::Counter::default()),
            OutMapperConfig::TLS(c) => {
                let a = tls::client::Client::new(c.host.as_str(), c.insecure.unwrap_or_default());
                Box::new(a)
            }
            OutMapperConfig::Socks5(c) => {
                let u = c.userpass.clone();
                let a = socks5::client::Client {
                    up: if u.is_empty() {
                        None
                    } else {
                        Some(ruci::user::PlainText::from(u))
                    },
                    use_earlydata: c.early_data.unwrap_or_default(),
                };
                Box::new(a)
            }
            OutMapperConfig::Trojan(pass) => {
                let a = trojan::client::Client::new(pass);
                Box::new(a)
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
                    InMapperConfig::Socks5(UserPassField {
                        userpass: None,
                        more: None,
                    }),
                ],
            }],
            outbounds: vec![OutMapperConfigChain {
                tag: String::from("todo!()"),
                chain: vec![OutMapperConfig::Direct],
            }],
            tag_route: None,
        };
        let toml = toml::to_string(&sc).expect("valid toml");
        println!("{:#}", toml);

        let toml: StaticConfig = toml::from_str(&toml).expect("valid toml");
        println!("{:#?}", toml);
    }
}
