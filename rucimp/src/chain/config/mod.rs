/*!
 * 链式配置
 */

#[cfg(feature = "lua")]
pub mod lua;

#[cfg(feature = "lua")]
pub mod dynamic;

use std::path::PathBuf;

use ruci::{
    map::{http, socks5, socks5http, tls, trojan, MapperSync, ToMapper},
    net,
};
use serde::{Deserialize, Serialize};

/// 静态配置中有初始化后即确定的listen/dial数量和行为
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct StaticConfig {
    pub inbounds: Vec<InMapperConfigChain>,
    pub outbounds: Option<Vec<OutMapperConfigChain>>,
}

impl StaticConfig {
    /// convert config chain to mapper chain
    pub fn get_inbounds(&self) -> Vec<Vec<Box<dyn MapperSync>>> {
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

                chain.last_mut().unwrap().set_is_tail_of_chain(true);
                chain
            })
            .collect();

        listens
    }

    /// convert config chain to mapper chain
    pub fn get_outbounds(&self) -> Vec<Vec<Box<dyn MapperSync>>> {
        match &self.outbounds {
            None => Vec::new(),

            Some(dials) => {
                let dials: Vec<_> = dials
                    .iter()
                    .map(|config_chain| {
                        let mut chain = config_chain
                            .chain
                            .iter()
                            .map(|mapper_config| {
                                let mut mapper = mapper_config.to_mapper();
                                mapper.set_chain_tag(
                                    config_chain.tag.as_ref().unwrap_or(&String::new()),
                                );
                                mapper
                            })
                            .collect::<Vec<_>>();

                        chain.last_mut().unwrap().set_is_tail_of_chain(true);
                        chain
                    })
                    .collect();
                dials
            }
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct InMapperConfigChain {
    tag: Option<String>,
    chain: Vec<InMapperConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct OutMapperConfigChain {
    tag: Option<String>,
    chain: Vec<OutMapperConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum InMapperConfig {
    Stdio(String),
    Listener(Listener),
    Adder(i8),
    Counter,
    TLS(TlsIn),
    Http(UserPass),
    Socks5(UserPass),
    Socks5Http(UserPass),
    Trojan(TrojanIn),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum OutMapperConfig {
    Direct,
    Blackhole,
    Stdio(String),
    Dialer(Dialer),
    Adder(i8),
    Counter,
    TLS(TlsOut),
    Socks5(Socks5Out),
    Trojan(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Dialer {
    TcpDialer(String),
    UnixDialer(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Listener {
    TcpListener(String),
    UnixListener(String),
}

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
pub struct UserPass {
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
            InMapperConfig::Stdio(s) => ruci::map::stdio::Stdio::from(s),
            InMapperConfig::Listener(lis) => match lis {
                Listener::TcpListener(tcp_l_str) => {
                    let a = net::Addr::from_ip_addr_str("tcp", tcp_l_str).unwrap();
                    Box::new(ruci::map::network::TcpStreamGenerator {
                        fixed_target_addr: Some(a),
                        ..Default::default()
                    })
                }
                Listener::UnixListener(_) => todo!(),
            },
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
                            .map(|up| ruci::user::UserPass::from(up.to_string()))
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
                            .map(|up| ruci::user::UserPass::from(up.to_string()))
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
                            .map(|up| ruci::user::UserPass::from(up.to_string()))
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
            OutMapperConfig::Stdio(s) => ruci::map::stdio::Stdio::from(s),
            OutMapperConfig::Blackhole => Box::new(ruci::map::network::BlackHole::default()),

            OutMapperConfig::Direct => Box::new(ruci::map::network::Direct::default()),
            OutMapperConfig::Dialer(d) => match d {
                Dialer::TcpDialer(td_str) => {
                    let a = net::Addr::from_ip_addr_str("tcp", td_str).unwrap();
                    Box::new(ruci::map::network::TcpDialer {
                        fixed_target_addr: Some(a),
                        ..ruci::map::network::TcpDialer::default()
                    })
                }
                Dialer::UnixDialer(_) => todo!(),
            },
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
                        Some(ruci::user::UserPass::from(u))
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
                    InMapperConfig::Listener(Listener::TcpListener("0.0.0.0:1080".to_string())),
                    InMapperConfig::Counter,
                    InMapperConfig::Socks5(UserPass {
                        userpass: None,
                        more: None,
                    }),
                ],
            }],
            outbounds: None,
        };
        let toml = toml::to_string(&sc).unwrap();
        println!("{:#}", toml);

        let toml: StaticConfig = toml::from_str(&toml).unwrap();
        println!("{:#?}", toml);
    }
}
