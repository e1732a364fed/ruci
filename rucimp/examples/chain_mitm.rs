/*!
演示 mitm 的 代理用法

 这里手动配置 StaticConfig, 不使用 加载配置文件的方式
*/

use rucimp::modes::chain::{
    config::{
        BindDialerConfig, DirectConfig, InMapConfig, InMapConfigChain, OutMapConfig,
        OutMapConfigChain, PlainTextSet, StaticConfig, TlsIn, TlsOut, TrojanPassSet,
    },
    engine::Engine,
};

async fn run_engine_server_end() -> anyhow::Result<()> {
    Engine::run_static_engine(StaticConfig {
        inbounds: vec![InMapConfigChain {
            tag: None,
            chain: vec![
                InMapConfig::Listener {
                    listen_addr: "127.0.0.1:10801".to_string(),
                    ext: None,
                },
                InMapConfig::TLS(TlsIn {
                    cert: "resource/test_ca_cert.pem".to_string(),
                    key: "resource/test_ca_key.pem".to_string(),
                    alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                }),
                InMapConfig::Trojan(TrojanPassSet::default()),
            ],
        }],
        outbounds: vec![OutMapConfigChain {
            tag: "direct_tls".to_string(),
            chain: vec![
                OutMapConfig::Direct(DirectConfig {
                    leak_target_addr: Some(true),
                    ..Default::default()
                }),
                OutMapConfig::TLS(TlsOut {
                    alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                    ..Default::default()
                }),
            ],
        }],
        tag_route: None,
        fallback_route: None,
        rule_route: None,
    })
    .await
}

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
                InMapConfig::Socks5Http(PlainTextSet::default()),
                InMapConfig::MITM(TlsIn {
                    cert: "resource/test_ca_cert.pem".to_string(),
                    key: "resource/test_ca_key.pem".to_string(),
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
                OutMapConfig::TLS(TlsOut {
                    host: Some("www.google.com".to_string()),
                    insecure: Some(true),
                    alpn: Some(vec!["h2".to_string(), "http/1.1".to_string()]),
                    ..Default::default()
                }),
                OutMapConfig::Trojan("".to_string()),
            ],
        }],
        tag_route: None,
        fallback_route: None,
        rule_route: None,
    };

    Engine::run_static_engine(sc).await
}
