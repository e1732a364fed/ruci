use std::path::PathBuf;

use crate::modes::suit;

use futures::executor::block_on;
use ruci::map::{network::Direct, *};

use super::LDConfig;

/// 将所有 in_map 从 名称映射到 MapBox.
///
/// 可作为 SuitEngine::new 的参数
pub fn load_in_maps_by_str_and_ld_config(s: &str, c: LDConfig) -> Option<MapBox> {
    match s {
        "adder" => {
            let a = ruci::map::math::Adder {
                add_num: c.number_arg.unwrap_or(1) as i8,
                ..Default::default()
            };
            Some(Box::new(a))
        }
        "counter" => {
            let a = ruci::map::counter::Counter::default();
            Some(Box::new(a))
        }
        "tls" => {
            let a = tls::server::Server::new(tls::server::ServerOptions {
                addr: "todo!()".to_string(),
                cert: PathBuf::from(c.cert.unwrap_or_default()),
                key: PathBuf::from(c.key.unwrap_or_default()),
                alpn: c.alpn,
            });
            Some(Box::new(a))
        }
        "socks5" => {
            let a = block_on(socks5::server::Server::new(
                suit::config::adapter::get_socks5_server_option_from_ld_config(c),
            ));
            Some(Box::new(a))
        }
        "http" => {
            let a = block_on(http_proxy::Server::new(
                suit::config::adapter::get_http_server_option_from_ld_config(c),
            ));
            Some(Box::new(a))
        }
        "socks5http" => {
            let a = block_on(socks5http::Server::new(
                suit::config::adapter::get_socks5http_server_option_from_ld_config(c),
            ));
            Some(Box::new(a))
        }
        "trojan" => {
            let a = block_on(trojan::server::Server::new(
                suit::config::adapter::get_trojan_server_option_from_ld_config(c),
            ));
            Some(Box::new(a))
        }

        _ => None,
    }
}

/// 将所有  out_map 从 名称映射到 MapBox.
///
/// 可作为 SuitEngine::new 的参数
pub fn load_out_maps_by_str_and_ld_config(s: &str, c: LDConfig) -> Option<MapBox> {
    match s {
        "direct" => Some(Box::<Direct>::default()),

        "adder" => {
            let a = ruci::map::math::Adder {
                add_num: c.number_arg.unwrap_or(1) as i8,
                ..Default::default()
            };
            Some(Box::new(a))
        }
        "counter" => {
            let a = ruci::map::counter::Counter::default();
            Some(Box::new(a))
        }

        "tls" => {
            let a = tls::client::Client::new(tls::client::ClientOptions {
                domain: c.host.unwrap_or_default(),
                is_insecure: c.insecure.unwrap_or_default(),
                alpn: c.alpn,
            });
            Some(Box::new(a))
        }

        "socks5" => {
            let u = c.uuid.unwrap_or_default();
            let a = socks5::client::Client {
                up: if u.is_empty() {
                    None
                } else {
                    Some(ruci::user::PlainText::from(u))
                },
                use_earlydata: c.early_data.unwrap_or_default(),
                ..Default::default()
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

pub fn get_socks5_server_option_from_ld_config(c: LDConfig) -> socks5::server::Config {
    let mut so = socks5::server::Config {
        user_whitespace_pass: c.uuid,
        ..Default::default()
    };
    let ruci_userpass = c.users.map(|up_v| {
        up_v.iter()
            .map(|up| ruci::user::PlainText::new(up.user.clone(), up.pass.clone()))
            .collect::<Vec<_>>()
    });
    so.user_passes = ruci_userpass;
    so
}

pub fn get_socks5_server_option_from_toml_config_str(toml_str: &str) -> socks5::server::Config {
    let c: LDConfig = toml::from_str(toml_str).expect("toml is valid");
    get_socks5_server_option_from_ld_config(c)
}

pub fn get_http_server_option_from_ld_config(c: LDConfig) -> http_proxy::Config {
    let mut so = http_proxy::Config {
        user_whitespace_pass: c.uuid,
        ..Default::default()
    };
    let ruci_userpass = c.users.map(|up_v| {
        up_v.iter()
            .map(|up| ruci::user::PlainText::new(up.user.clone(), up.pass.clone()))
            .collect::<Vec<_>>()
    });
    so.user_passes = ruci_userpass;
    so
}

pub fn get_socks5http_server_option_from_ld_config(c: LDConfig) -> socks5http::Config {
    let mut so = socks5http::Config {
        user_whitespace_pass: c.uuid,
        ..Default::default()
    };
    let ruci_userpass = c.users.map(|up_v| {
        up_v.iter()
            .map(|up| ruci::user::PlainText::new(up.user.clone(), up.pass.clone()))
            .collect::<Vec<_>>()
    });
    so.user_passes = ruci_userpass;
    so
}

pub fn get_trojan_server_option_from_ld_config(c: LDConfig) -> trojan::server::Config {
    let mut so = trojan::server::Config {
        pass: c.uuid,
        ..Default::default()
    };
    let ruci_userpass = c.users.map(|up_v| {
        up_v.iter()
            .map(|up| {
                if up.user.is_empty() {
                    up.pass.clone()
                } else {
                    up.user.clone()
                }
            })
            .collect::<Vec<_>>()
    });
    so.passes = ruci_userpass;
    so
}
