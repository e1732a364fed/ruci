use std::path::PathBuf;

use crate::suit;

use futures::executor::block_on;
use ruci::map::*;

use super::LDConfig;

/// 将所有在 in_mapper 从 名称映射到 MapperBox.
///
/// 可作为 SuitEngine::new 的参数
pub fn load_in_mappers_by_str_and_ldconfig(s: &str, c: LDConfig) -> Option<MapperBox> {
    match s {
        "adder" => {
            let a = ruci::map::math::Adder {
                addnum: c.number_arg.unwrap_or(1) as i8,
            };
            Some(Box::new(a))
        }
        "counter" => {
            let a = ruci::map::counter::Counter;
            Some(Box::new(a))
        }
        "tls" => {
            let a = tls::server::Server::new(tls::server::ServerOptions {
                addr: "todo!()".to_string(),
                cert: PathBuf::from(c.cert.unwrap_or_default()),
                key: PathBuf::from(c.key.unwrap_or_default()),
            });
            Some(Box::new(a))
        }
        "socks5" => {
            let a = block_on(socks5::server::Server::new(
                suit::config::adapter::get_socks5_server_option_from_ldconfig(c),
            ));
            Some(Box::new(a))
        }
        "trojan" => {
            let a = block_on(trojan::server::Server::new(
                suit::config::adapter::get_trojan_server_option_from_ldconfig(c),
            ));
            Some(Box::new(a))
        }

        _ => None,
    }
}

/// 将所有在本包中实现的 out_mapper 从 名称映射到 MapperBox.
///
/// 可作为 SuitEngine::new 的参数
pub fn load_out_mappers_by_str_and_ldconfig(s: &str, c: LDConfig) -> Option<MapperBox> {
    match s {
        "adder" => {
            let a = ruci::map::math::Adder {
                addnum: c.number_arg.unwrap_or(1) as i8,
            };
            Some(Box::new(a))
        }
        "counter" => {
            let a = ruci::map::counter::Counter;
            Some(Box::new(a))
        }

        "tls" => {
            let a = tls::client::Client::new(
                c.host.unwrap_or_default().as_str(),
                c.insecure.unwrap_or_default(),
            );
            Some(Box::new(a))
        }

        "socks5" => {
            let u = c.uuid.unwrap_or_default();
            let a = socks5::client::Client {
                up: if u == "" {
                    None
                } else {
                    Some(ruci::user::UserPass::from(u))
                },
                use_earlydata: c.early_data.unwrap_or_default(),
            };
            Some(Box::new(a))
        }

        "trojan" => {
            let u = c.uuid.unwrap_or_default();
            let a = trojan::client::Client::new(&u);
            Some(Box::new(a))
        }

        _ => None,
    }
}

pub fn get_socks5_server_option_from_ldconfig(c: LDConfig) -> socks5::server::Config {
    let mut so = socks5::server::Config::default();
    so.user_whitespace_pass = c.uuid;
    let ruci_userpass = c.users.map_or(None, |up_v| {
        Some(
            up_v.iter()
                .map(|up| ruci::user::UserPass::new(up.user.clone(), up.pass.clone()))
                .collect::<Vec<_>>(),
        )
    });
    so.user_passes = ruci_userpass;
    so
}

pub fn get_socks5_server_option_from_toml_config_str(toml_str: &str) -> socks5::server::Config {
    let c: LDConfig = toml::from_str(toml_str).unwrap();
    get_socks5_server_option_from_ldconfig(c)
}

pub fn get_trojan_server_option_from_ldconfig(c: LDConfig) -> trojan::server::Config {
    let mut so = trojan::server::Config::default();
    so.pass = c.uuid;
    let ruci_userpass = c.users.map_or(None, |up_v| {
        Some(
            up_v.iter()
                .map(|up| {
                    if up.user == "" {
                        up.pass.clone()
                    } else {
                        up.user.clone()
                    }
                })
                .collect::<Vec<_>>(),
        )
    });
    so.passes = ruci_userpass;
    so
}
