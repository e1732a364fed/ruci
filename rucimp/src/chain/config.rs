/*!
 *
 */

#[cfg(feature = "lua")]
pub mod lua;

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
    Trojan(TrojanOut),
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
pub struct Socks5Out(Option<String>);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrojanIn {
    password: Option<String>,
    more: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrojanOut(String);

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
