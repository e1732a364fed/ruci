/*!
演示 mitm 的 代理用法

 这里手动配置 StaticConfig, 不使用 加载配置文件的方式
*/

use ruci_tls::server::TlsServerOptions;
use rucimp::modes::chain::{config::*, engine::Engine};

mod shared;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    shared::print_env_version_and_init_log("example: chain mitm");

    tokio::spawn(run_engine_server_end());

    let sc = StaticConfig {
        inbounds: vec![InMapConfigChain {
            tag: None,
            chain: vec![
                InMapConfig::Listener {
                    listen_addr: "127.0.0.1:10800".to_string(),
                    ext: None,
                },
                InMapConfig::Socks5Http(PlainTextPassSet::default()),
                InMapConfig::MITM(TlsServerOptions {
                    cert: "test_ca_cert.pem".into(),
                    key: "test_ca_key.pem".into(),
                    alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                }),
            ],
        }],
        outbounds: vec![OutMapConfigChain {
            tag: "mitm_dail_trojan".to_string(),
            chain: vec![
                OutMapConfig::BindDialer(Box::new(BindDialerConfig {
                    dial_addr: Some("127.0.0.1:10801".to_string()),
                    ..Default::default()
                })),
                OutMapConfig::TLS(ruci_tls::client::TlsClientOptions {
                    host: Some("www.google.com".to_string()),
                    insecure: true,
                    alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                }),
                OutMapConfig::Trojan("".to_string()),
            ],
        }],
        ..Default::default()
    };

    Engine::new_and_run_static(sc).await
}

async fn run_engine_server_end() -> anyhow::Result<()> {
    let sc = StaticConfig {
        inbounds: vec![InMapConfigChain {
            tag: None,
            chain: vec![
                InMapConfig::Listener {
                    listen_addr: "127.0.0.1:10801".to_string(),
                    ext: None,
                },
                InMapConfig::TLS(TlsServerOptions {
                    cert: "test_ca_cert.pem".into(),
                    key: "test_ca_key.pem".into(),
                    alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                }),
                InMapConfig::Trojan(ruci::map::trojan::server::Config::default()),
            ],
        }],
        outbounds: vec![OutMapConfigChain {
            tag: "direct_tls".to_string(),
            chain: vec![
                OutMapConfig::Direct(DirectConfig {
                    leak_target_addr: Some(true),
                    ..Default::default()
                }),
                OutMapConfig::TLS(ruci_tls::client::TlsClientOptions {
                    alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                    ..Default::default()
                }),
            ],
        }],
        ..Default::default()
    };
    Engine::new_and_run_static(sc).await
}
