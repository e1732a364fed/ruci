/*!
 * 链式配置
 */

#[cfg(feature = "lua")]
pub mod lua;

use std::path::PathBuf;

use ruci::map::{socks5, tls, trojan, MapperSync, ToMapper};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct StaticConfig {
    pub listen: Vec<InMapperConfigChain>,
    pub dial: Option<Vec<OutMapperConfigChain>>,
}

impl StaticConfig {
    /// convert config chain to mapper chain
    pub fn get_listens(&self) -> Vec<Vec<Box<dyn MapperSync>>> {
        let listens: Vec<_> = self
            .listen
            .iter()
            .map(|config_chain| {
                config_chain
                    .chain
                    .iter()
                    .map(|mapper_config| mapper_config.to_mapper())
                    .collect::<Vec<_>>()
            })
            .collect();
        listens
    }

    /// convert config chain to mapper chain
    pub fn get_dials(&self) -> Vec<Vec<Box<dyn MapperSync>>> {
        match &self.dial {
            None => Vec::new(),

            Some(dials) => {
                let dials: Vec<_> = dials
                    .iter()
                    .map(|config_chain| {
                        config_chain
                            .chain
                            .iter()
                            .map(|mapper_config| mapper_config.to_mapper())
                            .collect::<Vec<_>>()
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
    Listener(Listener),
    Adder(i8),
    Counter,
    TLS(TlsIn),
    Socks5(Socks5In),
    Trojan(TrojanIn),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum OutMapperConfig {
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
pub struct Socks5In {
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
            InMapperConfig::Listener(_) => todo!(),
            InMapperConfig::Adder(i) => i.to_mapper(),
            InMapperConfig::Counter => Box::new(ruci::map::counter::Counter),
            InMapperConfig::TLS(c) => tls::server::ServerOptions {
                addr: "todo!()".to_string(),
                cert: PathBuf::from(c.cert.clone()),
                key: PathBuf::from(c.key.clone()),
            }
            .to_mapper(),
            InMapperConfig::Socks5(c) => {
                let mut so = socks5::server::Config::default();
                so.user_whitespace_pass = c.userpass.clone();
                let ruci_userpass = c.more.as_ref().map_or(None, |up_v| {
                    Some(
                        up_v.iter()
                            .map(|up| ruci::user::UserPass::from(up.to_string()))
                            .collect::<Vec<_>>(),
                    )
                });
                so.user_passes = ruci_userpass;

                so.to_mapper()
            }
            InMapperConfig::Trojan(c) => {
                let mut so = trojan::server::Config::default();
                so.pass = c.password.clone();
                let ruci_userpass = c.more.as_ref().map_or(None, |up_v| {
                    Some(up_v.iter().map(|up| up.clone()).collect::<Vec<_>>())
                });
                so.passes = ruci_userpass;

                so.to_mapper()
            }
        }
    }
}

impl ToMapper for OutMapperConfig {
    fn to_mapper(&self) -> ruci::map::MapperBox {
        match self {
            OutMapperConfig::Dialer(_) => todo!(),
            OutMapperConfig::Adder(i) => i.to_mapper(),
            OutMapperConfig::Counter => Box::new(ruci::map::counter::Counter),
            OutMapperConfig::TLS(c) => {
                let a = tls::client::Client::new(c.host.as_str(), c.insecure.unwrap_or_default());
                Box::new(a)
            }
            OutMapperConfig::Socks5(c) => {
                let u = c.userpass.clone();
                let a = socks5::client::Client {
                    up: if u == "" {
                        None
                    } else {
                        Some(ruci::user::UserPass::from(u))
                    },
                    use_earlydata: c.early_data.unwrap_or_default(),
                };
                Box::new(a)
            }
            OutMapperConfig::Trojan(pass) => {
                let a = trojan::client::Client::new(&pass);
                Box::new(a)
            }
        }
    }
}

#[cfg(test)]
mod test {

    use super::*;
    #[test]
    fn serialize() {
        let sc = StaticConfig {
            listen: vec![InMapperConfigChain {
                tag: None,
                chain: vec![
                    InMapperConfig::Listener(Listener::TcpListener("0.0.0.0:1080".to_string())),
                    InMapperConfig::Counter,
                    InMapperConfig::Socks5(Socks5In {
                        userpass: None,
                        more: None,
                    }),
                ],
            }],
            dial: None,
        };
        let toml = toml::to_string(&sc).unwrap();
        println!("{:#}", toml);

        let toml: StaticConfig = toml::from_str(&toml).unwrap();
        println!("{:#?}", toml);
    }
}