/*!
演示 mitm 的 代理用法

 这里手动配置 StaticConfig, 不使用 加载配置文件的方式
*/

use std::collections::BTreeMap;

use rucimp::modes::chain::{config::*, engine::Engine};

mod shared;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    shared::print_env_version_and_init_log("example: chain mitm");

    tokio::spawn(run_engine_server_end());

    let c1 = vec![
        InMapConfig::Listener {
            listen_addr: "127.0.0.1:10800".to_string(),
            ext: None,
        },
        InMapConfig::Socks5Http(PlainTextPassSet::default()),
        InMapConfig::MITM(ruci::map::tls_config::ServerOptions {
            cert: "test_ca_cert.pem".to_string(),
            key: "test_ca_key.pem".to_string(),
            alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
        }),
    ];

    let mut inbounds = BTreeMap::new();
    inbounds.insert("c1".to_string(), c1);

    let c2 = vec![
        OutMapConfig::BindDialer(Box::new(BindDialerConfig {
            dial_addr: Some("127.0.0.1:10801".to_string()),
            ..Default::default()
        })),
        OutMapConfig::TLS(ruci::map::tls_config::ClientOptions {
            host: Some("www.google.com".to_string()),
            insecure: true,
            alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
            ..Default::default()
        }),
        OutMapConfig::Trojan(ruci::map::trojan::client::Config {
            password: Some("mypassword".to_string()),
            ..Default::default()
        }),
    ];
    let mut outbounds = BTreeMap::new();
    outbounds.insert("mitm_dail_trojan".to_string(), c2);

    let sc = StaticConfig {
        inbounds,
        outbounds,
        ..Default::default()
    };

    Engine::new_and_run_static(sc).await
}

async fn run_engine_server_end() -> anyhow::Result<()> {
    let mut inbounds = BTreeMap::new();
    inbounds.insert(
        "c1".to_string(),
        vec![
            InMapConfig::Listener {
                listen_addr: "127.0.0.1:10801".to_string(),
                ext: None,
            },
            InMapConfig::TLS(ruci::map::tls_config::ServerOptions {
                cert: "test_ca_cert.pem".into(),
                key: "test_ca_key.pem".into(),
                alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
            }),
            InMapConfig::Trojan(ruci::map::trojan::server::Config::default()),
        ],
    );

    let mut outbounds = BTreeMap::new();
    outbounds.insert(
        "direct_tls".to_string(),
        vec![
            OutMapConfig::Direct(DirectConfig {
                leak_target_addr: Some(true),
                ..Default::default()
            }),
            OutMapConfig::TLS(ruci::map::tls_config::ClientOptions {
                alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                ..Default::default()
            }),
        ],
    );

    let sc = StaticConfig {
        inbounds,
        outbounds,
        ..Default::default()
    };
    Engine::new_and_run_static(sc).await
}
