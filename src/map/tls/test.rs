use std::{env::set_var, sync::Arc, time::Duration};

use crate::{
    map::{tls, MapParams, Mapper, CID},
    net::{self, gen_random_higher_port, helpers::MockTcpStream},
};
use futures::{join, FutureExt};
use log::info;
use parking_lot::Mutex;
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    task,
};

use super::*;

#[should_panic]
#[tokio::test]
async fn dial_tls_in_mem() {
    set_var("RUST_LOG", "debug");
    let _ = env_logger::try_init();

    let writev = Arc::new(Mutex::new(Vec::new()));
    let client_tcps = MockTcpStream {
        read_data: vec![111, 222, 123],
        write_data: Vec::new(),
        write_target: Some(writev),
    };

    let a = tls::client::Client::new("www.baidu.com", true);
    let ta = net::Addr::from_strs("tcp", "", "1.2.3.4", 443).unwrap();
    println!("will out, {}", ta);

    //should panic here , as the data 111,222,123 is not cool for a server tls response.
    let r = a
        .maps(
            CID::default(),
            ProxyBehavior::ENCODE,
            MapParams {
                c: map::Stream::TCP(Box::new(client_tcps)),
                a: Some(ta),
                b: None,
                d: None,
                shutdown_rx: None,
            },
        )
        .await
        .c;

    let mut buf = [0u8; 1024];
    let n = r
        .try_unwrap_tcp()
        .unwrap()
        .read(&mut buf[..])
        .await
        .unwrap();

    println!("{}, {:?}", n, &buf[..n]);
}

async fn dial_future(listen_host_str: &str, listen_port: u16) -> std::io::Result<()> {
    tokio::time::sleep(Duration::from_secs(1)).await;
    let cs = TcpStream::connect((listen_host_str, listen_port))
        .await
        .unwrap();

    let a = tls::client::Client::new("test.domain", true);
    let ta = net::Addr::from_strs("tcp", "", "1.2.3.4", 443)?; //not used in our test, but required by the method.

    info!("client will out, {}", ta);
    let r = a
        .maps(
            CID::default(),
            ProxyBehavior::ENCODE,
            MapParams {
                c: map::Stream::TCP(Box::new(cs)),
                a: Some(ta),
                b: None,
                d: None,
                shutdown_rx: None,
            },
        )
        .await
        .c;

    info!("client ok");

    let mut buf = [3u8; 3];

    let mut r = r.try_unwrap_tcp()?;
    let r = r.as_mut();
    let n = r.write(&mut buf[..]).await?;
    r.flush().await?;

    assert_eq!(n, buf.len());
    info!("client has write byte num {},", n);

    Ok(())
}

async fn listen_future(listen_host_str: &str, listen_port: u16) -> std::io::Result<()> {
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/resource"))?;

    let mut path = PathBuf::new();
    path.push("test.crt");

    let mut path2 = PathBuf::new();
    path2.push("test.key");

    let a = tls::server::Server::new(tls::server::ServerOptions {
        addr: "todo!()".to_string(),
        cert: path,
        key: path2,
    });

    let listener = TcpListener::bind(listen_host_str.to_string() + ":" + &listen_port.to_string())
        .await
        .unwrap();

    let (nc, _raddr) = listener.accept().await?;

    info!("server will start");

    let add_result = a
        .maps(
            CID::default(),
            ProxyBehavior::DECODE,
            MapParams::new(Box::new(nc)),
        )
        .await;
    if let Some(e) = add_result.e {
        return Err(e);
    }
    let conn = add_result.c;

    debug!("tls listen");

    info!("server ok");

    let mut buf = [0u8; 1024];

    let n = conn.try_unwrap_tcp()?.read(&mut buf[..]).await?;

    info!("server read {}, {:?}", n, &buf[..n]);
    tokio::time::sleep(Duration::from_secs(1)).await;

    Ok(())
}

#[tokio::test]
async fn tls_local_loopback() -> std::io::Result<()> {
    set_var("RUST_LOG", "debug");
    let _ = env_logger::try_init();

    let listen_ip = "127.0.0.1";
    let p = gen_random_higher_port();

    let lf = listen_future(listen_ip, p).fuse();
    let df = dial_future(listen_ip, p).fuse();

    let h1 = task::spawn(lf);
    let h2 = task::spawn(df);

    let r = join!(h1, h2);

    info!("join end, result: {:?}", r);
    Ok(())
}
