use futures::join;
use log::info;
use ruci::{
    map::{socks5, tls, MapParams, Mapper},
    net,
    user::UserPass,
};
use rucimp::{load_in_adder_by_str, load_out_adder_by_str, suit::config::Config, SuitEngine};
use std::{env::set_var, io};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task,
};

const TARGET_PORT: u16 = 80;
const TARGET_NAME: &str = "www.baidu.com";

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
) -> io::Result<()> {
    info!("start run f_dial_future, {}", rid);
    let cs = TcpStream::connect((listen_host_str, listen_port))
        .await
        .unwrap();

    let mut readbuf = [0u8; 1024];

    let a = tls::Client::new("do.main", true);

    let ta = net::Addr::from_strs("tcp", the_target_name, "", the_target_port)?;
    let nc = a
        .maps(
            0,
            ruci::map::ProxyBehavior::ENCODE,
            MapParams::ca(Box::new(cs), ta.clone()),
        )
        .await
        .c
        .unwrap();

    let a = socks5::client::Client {
        up: Some(UserPass::from("u0 p0".to_string())),
        use_earlydata: false,
    };
    let mut newconn = a
        .maps(0, ruci::map::ProxyBehavior::ENCODE, MapParams::ca(nc, ta))
        .await
        .c
        .unwrap();

    info!("client{} writing hello...", rid,);

    newconn.write(&b"hello\n"[..]).await.unwrap();

    info!("client{} reading...", rid,);

    let n = newconn.read(&mut readbuf[..]).await.unwrap();
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
async fn suit_engine_socks5_tls_direct_and_outadder() -> std::io::Result<()> {
    set_var("RUST_LOG", "debug");
    let _ = env_logger::try_init();
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../resource"))?;

    let (ws, port) = get_lconfig_str();
    let c: Config = toml::from_str(&ws).unwrap();

    let mut se = SuitEngine::new(load_in_adder_by_str, load_out_adder_by_str);
    se.load_config(rucimp::Config { proxy_config: c });

    let listen_future = async move {
        info!("try start listen");

        let r = se.run().await;

        info!("r {:?}", r);
    };

    let listen_handle = task::spawn(listen_future);
    let dialh = task::spawn(f_dial_future_tls_out_adder(
        0,
        "127.0.0.1",
        port,
        TARGET_NAME,
        TARGET_PORT,
    ));

    let x = join!(listen_handle, dialh);

    info!("end, {:?}", x);
    Ok(())
}
