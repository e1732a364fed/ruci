/*!
在 working dir 或 working dir /resource 或 ../resource/ 文件夹查找 local.lua 或
 用户提供的参数作为配置文件 读取它并以 chain 模式运行
*/

use std::time::Duration;

use rucimp::{
    modes::chain::{
        config::{
            BindDialerConfig, DirectConfig, InMapConfig, InMapConfigChain, OutMapConfig,
            OutMapConfigChain, PlainTextSet, StaticConfig, TlsIn, TlsOut, TrojanPassSet,
        },
        engine::Engine,
    },
    utils::*,
};

// 这里手动配置 StaticConfig, 不使用 加载配置文件的方式

async fn run_engine2() -> anyhow::Result<()> {
    let mut e = Engine::new();

    e.init_static(StaticConfig {
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
    });

    let mut js = e.run().await?;

    wait_close_sig().await?;

    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(3));
        println!("Force shutdown after 3 secs!"); //only println works at this point.
        std::process::exit(1);
    });

    e.stop().await;

    debug!("Waiting for join set");

    let r = js.shutdown().await;

    debug!("{:?}", r);

    Ok(())
}

use tracing::debug;
mod shared;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    shared::print_env_version_and_init_log("example: chain mitm");

    tokio::spawn(run_engine2());

    let mut e = Engine::new();

    e.init_static(StaticConfig {
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
    });

    let mut js = e.run().await?;

    wait_close_sig().await?;

    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(3));
        println!("Force shutdown after 3 secs!"); //only println works at this point.
        std::process::exit(1);
    });

    e.stop().await;

    debug!("Waiting for join set");

    let r = js.shutdown().await;

    debug!("{:?}", r);

    Ok(())
}
