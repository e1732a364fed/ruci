/*!
 *
 */

#[cfg(feature = "lua")]
pub mod lua;

use std::path::PathBuf;

use ruci::map::{socks5, tls, trojan, ToMapper};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct StaticConfig {
    pub listen: Vec<InMapperStruct>,
    pub dial: Option<Vec<OutMapperStruct>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct InMapperStruct {
    tag: Option<String>,
    chain: Vec<InMapper>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct OutMapperStruct {
    tag: Option<String>,
    chain: Vec<OutMapper>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum InMapper {
    Listener(Listener),
    Adder(i8),
    Counter,
    TLS(TlsIn),
    Socks5(Socks5In),
    Trojan(TrojanIn),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum OutMapper {
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
    insecure: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Socks5In {
    userpass: Option<String>,
    more: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Socks5Out {
    userpass: String,
    early_data: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrojanIn {
    password: Option<String>,
    more: Option<Vec<String>>,
}

impl ToMapper for InMapper {
    fn to_mapper(&self) -> ruci::map::MapperBox {
        match self {
            InMapper::Listener(_) => todo!(),
            InMapper::Adder(i) => i.to_mapper(),
            InMapper::Counter => Box::new(ruci::map::counter::Counter),
            InMapper::TLS(c) => tls::server::ServerOptions {
                addr: "todo!()".to_string(),
                cert: PathBuf::from(c.cert.clone()),
                key: PathBuf::from(c.key.clone()),
            }
            .to_mapper(),
            InMapper::Socks5(c) => {
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
            InMapper::Trojan(c) => {
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

impl ToMapper for OutMapper {
    fn to_mapper(&self) -> ruci::map::MapperBox {
        match self {
            OutMapper::Dialer(_) => todo!(),
            OutMapper::Adder(i) => i.to_mapper(),
            OutMapper::Counter => Box::new(ruci::map::counter::Counter),
            OutMapper::TLS(c) => {
                let a = tls::client::Client::new(c.host.as_str(), c.insecure);
                Box::new(a)
            }
            OutMapper::Socks5(c) => {
                let u = c.userpass.clone();
                let a = socks5::client::Client {
                    up: if u == "" {
                        None
                    } else {
                        Some(ruci::user::UserPass::from(u))
                    },
                    use_earlydata: c.early_data,
                };
                Box::new(a)
            }
            OutMapper::Trojan(pass) => {
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
            listen: vec![InMapperStruct {
                tag: None,
                chain: vec![
                    InMapper::Listener(Listener::TcpListener("0.0.0.0:1080".to_string())),
                    InMapper::Counter,
                    InMapper::Socks5(Socks5In {
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
