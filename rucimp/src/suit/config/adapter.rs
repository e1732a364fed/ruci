use super::*;
use ruci::map::*;

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
