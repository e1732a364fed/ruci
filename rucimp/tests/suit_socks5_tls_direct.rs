use crate::net::CID;
use futures::FutureExt;
use parking_lot::Mutex;
use ruci::{
    map::{
        socks5,
        tls::{self, client::ClientOptions},
        MapParams, Mapper,
    },
    net,
    user::PlainText,
};
use rucimp::modes::suit::config::{
    adapter::{load_in_mappers_by_str_and_ld_config, load_out_mappers_by_str_and_ld_config},
    Config,
};
use std::{env::set_var, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tracing::info;

const TARGET_PORT: u16 = 80;
const TARGET_NAME: &str = "www.baidu.com";

fn init_log() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    let _ = tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init();
}

fn get_lconfig_str() -> (String, u16) {
    let toml_str = r#"
    [[listen]]
    protocol = "socks5"
    host = "127.0.0.1"
    port = 12345
    uuid = "u0 p0"
    users = [ { user = "u1", pass = "p1"},  { user = "u2", pass = "p2"}, ]
    tls = true
    cert = "test.crt"
    key = "test.key"

    [[dial]]
    protocol = "direct"
    "#;

    let p = net::gen_random_higher_port();
    let ps = p.to_string();
    let toml_str = toml_str.replace("12345", &ps);

    (toml_str, p)
}

async fn f_dial_future_tls_out_adder(
    rid: i32,
    listen_host_str: &str,
    listen_port: u16,
    the_target_name: &str,
    the_target_port: u16,
) -> anyhow::Result<()> {
    tokio::time::sleep(Duration::from_millis(400)).await;
    info!("start run f_dial_future, {}", rid);

    let cs = TcpStream::connect((listen_host_str, listen_port))
        .await
        .unwrap();

    let mut readbuf = [0u8; 1024];

    let a = tls::client::Client::new(ClientOptions {
        domain: "do.main".to_string(),
        is_insecure: true,
        ..Default::default()
    });

    let cid = CID::default();

    let ta = net::Addr::from_strs("tcp", the_target_name, "", the_target_port)?;
    let nc = a
        .maps(
            cid.clone(),
            ruci::map::ProxyBehavior::ENCODE,
            MapParams::ca(Box::new(cs), ta.clone()),
        )
        .await
        .c
        .try_unwrap_tcp()
        .expect("last_result as c");

    let a = socks5::client::Client {
        up: Some(PlainText::from("u0 p0".to_string())),
        use_earlydata: false,
    };
    let mut newconn = a
        .maps(
            cid.clone(),
            ruci::map::ProxyBehavior::ENCODE,
            MapParams::ca(nc, ta),
        )
        .await
        .c
        .try_unwrap_tcp()
        .expect("last_result as c");

    info!("client{} writing hello...", rid,);

    newconn.write(&b"hello\n"[..]).await?;

    info!("client{} reading...", rid,);

    let n = newconn.read(&mut readbuf[..]).await?;
    info!("client{} read, {:?}", rid, &readbuf[..n]);
    info!(
        "client{} read str is, {:?}",
        rid,
        String::from_utf8_lossy(&readbuf[..n])
    );

    // info!("client will close...",);

    // let _ =  cs.shutdown(std::net::Shutdown::Both);
    // info!("client closed");

    Ok(())
}

#[tokio::test]
async fn suit_engine_socks5_tls_direct_and_outadder() -> anyhow::Result<()> {
    set_var("RUST_LOG", "debug");
    init_log();
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../resource"))?;

    let (ws, port) = get_lconfig_str();
    let c: Config = toml::from_str(&ws).unwrap();

    let se = rucimp::modes::suit::engine::SuitEngine::default();
    let se = Arc::new(Mutex::new(Box::new(se)));
    let sec = se.clone();

    se.lock().load_config(
        c,
        load_in_mappers_by_str_and_ld_config,
        load_out_mappers_by_str_and_ld_config,
    );

    let se = &se;
    //注意，不用 借用的话，下面的 move 会 转移所有权，导致在非阻塞的 listen_future
    // 刚退出就会执行 drop(se), 进而将其内部储存的tx drop掉，进而关闭监听，导致失败

    let listen_future = async move {
        info!("try start listen");

        let r = se.lock().run().await;

        info!("listenr {:?}", r);
    };

    let listen_future = listen_future.fuse();
    let dialh = f_dial_future_tls_out_adder(0, "127.0.0.1", port, TARGET_NAME, TARGET_PORT).fuse();

    futures::pin_mut!(listen_future, dialh);

    loop {
        futures::select! {

            r = dialh => {
                info!("dial finished first, will return ,{:?}, {:?}",r, sec.lock().gtr);
                tokio::time::sleep(Duration::from_millis(400)).await;
                info!("dial finished first ,print again, {:?}",sec.lock().gtr);

                break;
            },
            () = listen_future => {
                info!("listen finished first");
            },
        }
    }

    info!("end,",);
    Ok(())
}

#[tokio::test]
async fn suit_engine2_socks5_tls_direct_and_outadder() -> anyhow::Result<()> {
    set_var("RUST_LOG", "debug");
    init_log();
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../resource"))?;

    let (ws, port) = get_lconfig_str();
    let c: Config = toml::from_str(&ws).unwrap();

    let se = rucimp::modes::suit::engine::SuitEngine::default();
    let se = Arc::new(Mutex::new(Box::new(se)));
    let sec = se.clone();

    se.lock().load_config(
        c,
        load_in_mappers_by_str_and_ld_config,
        load_out_mappers_by_str_and_ld_config,
    );

    //注意，不用 借用的话，下面的 move 会 转移所有权，导致在非阻塞的 listen_future
    // 刚退出就会执行 drop(se), 进而将其内部储存的tx drop掉，进而关闭监听，导致失败

    let listen_future = async {
        info!("try start listen");

        let r = se.lock().run().await;

        info!("listenr {:?}", r);
    };

    let listen_future = listen_future.fuse();
    let dialh = f_dial_future_tls_out_adder(0, "127.0.0.1", port, TARGET_NAME, TARGET_PORT).fuse();

    futures::pin_mut!(listen_future, dialh);

    loop {
        futures::select! {

            r = dialh => {
                info!("dial finished first, will return ,{:?}, {:?}",r, sec.lock().gtr);
                tokio::time::sleep(Duration::from_millis(400)).await;
                info!("dial finished first ,print again, {:?}",sec.lock().gtr);

                break;
            },
            () = listen_future => {
                info!("listen finished first");
            },
        }
    }

    info!("end,",);
    Ok(())
}
