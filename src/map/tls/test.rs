use std::{env::set_var, sync::Arc, time::Duration};

use async_std::{
    io::ReadExt,
    net::{TcpListener, TcpStream},
    sync::Mutex,
    task,
};
use async_std_test::async_test;
use futures::{join, FutureExt};
use log::info;

use crate::{
    map::{tls, MapParams, Mapper},
    net::{self, gen_random_higher_port, helpers::MockTcpStream},
};

use super::*;

#[should_panic]
#[async_test]
async fn tls_in_mem() -> std::io::Result<()> {
    set_var("RUST_LOG", "debug");
    let _ = env_logger::try_init();

    let writev = Arc::new(Mutex::new(Vec::new()));
    //let writevc = writev.clone();
    let client_tcps = MockTcpStream {
        read_data: vec![111, 222, 123],
        write_data: Vec::new(),
        write_target: Some(writev),
    };

    let a = tls::Client::new("www.baidu.com", true);
    let ta = net::Addr::from_strs("tcp", "", "1.2.3.4", 443)?;
    println!("will out, {}", ta);

    //should panic here , as the data 111,222,123 is not cool for a server tls response.
    let mut r = a
        .maps(
            0,
            ProxyBehavior::ENCODE,
            MapParams {
                c: map::Stream::TCP(Box::new(client_tcps)),
                a: Some(ta),
                b: None,
                d: None,
            },
        )
        .await
        .c
        .unwrap();

    let mut buf = [0u8; 1024];
    let n = r.read(&mut buf[..]).await?;

    println!("{}, {:?}", n, &buf[..n]);

    Ok(())
}

async fn dial_future(listen_host_str: &str, listen_port: u16) -> std::io::Result<()> {
    task::sleep(Duration::from_secs(1)).await;
    let cs = TcpStream::connect((listen_host_str, listen_port))
        .await
        .unwrap();

    let a = tls::Client::new("test.domain", true);
    let ta = net::Addr::from_strs("tcp", "", "1.2.3.4", 443)?; //not used in our test, but required by the method.

    info!("client will out, {}", ta);
    let mut r = a
        .maps(
            0,
            ProxyBehavior::ENCODE,
            MapParams {
                c: map::Stream::TCP(Box::new(cs)),
                a: Some(ta),
                b: None,
                d: None,
            },
        )
        .await
        .c
        .unwrap();

    info!("client ok");

    let mut buf = [3u8; 3];

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

    let a = tls::Server::new(ServerOptions {
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
        .maps(0, ProxyBehavior::DECODE, MapParams::new(Box::new(nc)))
        .await;
    if let Some(e) = add_result.e {
        return Err(e);
    }
    let mut conn = add_result.c.unwrap();

    debug!("tls listen");

    info!("server ok");

    let mut buf = [0u8; 1024];

    let n = conn.read(&mut buf[..]).await?;

    info!("server read {}, {:?}", n, &buf[..n]);
    task::sleep(Duration::from_secs(1)).await;

    Ok(())
}

#[async_test]
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
