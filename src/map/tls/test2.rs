use self::map::MapParams;

use std::{io, path::PathBuf};

use crate::{
    map::{self, *},
    net::{self, ConnTrait, CID},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

async fn dial_future(
    listen_host_str: &str,
    listen_port: u16,
) -> std::io::Result<Box<dyn ConnTrait>> {
    let cs = TcpStream::connect((listen_host_str, listen_port))
        .await
        .unwrap();

    let a = super::client::Client::new("test.domain", true);
    let ta = net::Addr::from_strs("tcp", "", "1.2.3.4", 443)?; //not used in our test, but required by the method.

    let r = a
        .maps(
            CID::default(),
            ProxyBehavior::ENCODE,
            MapParams {
                c: crate::net::Stream::TCP(Box::new(cs)),
                a: Some(ta),
                b: None,
                d: None,
                shutdown_rx: None,
            },
        )
        .await
        .c;

    let r = r.try_unwrap_tcp()?;

    Ok(r)
}

#[allow(unused)]
async fn listen_future(listen_host_str: &str, listen_port: u16) -> std::io::Result<()> {
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/resource"))?;

    let mut path = PathBuf::new();
    path.push("test.crt");

    let mut path2 = PathBuf::new();
    path2.push("test.key");

    let a = super::server::Server::new(super::server::ServerOptions {
        addr: "todo!()".to_string(),
        cert: path,
        key: path2,
    });

    let listener = TcpListener::bind(listen_host_str.to_string() + ":" + &listen_port.to_string())
        .await
        .expect("listener bind failed");

    tokio::spawn(async move {
        let (nc, _raddr) = listener.accept().await?;

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

        let mut c = conn.try_unwrap_tcp()?;

        //let mut buf = BytesMut::zeroed(1024);
        loop {
            unsafe {
                c.read(&mut VEC1).await?;
            }
        }
        Ok::<(), io::Error>(())
    });

    Ok(())
}

pub async fn test_init() -> std::io::Result<Box<dyn ConnTrait>> {
    const HOST: &str = "127.0.0.1";
    const PORT: u16 = 23456;

    listen_future(HOST, PORT).await?;

    dial_future(HOST, PORT).await
}

pub async fn test_adder_r(l: usize) -> std::io::Result<()> {
    let mut d = test_init().await?;
    for _ in 0..l {
        unsafe {
            d.write(&VEC2).await?;
        }
    }

    Ok(())
}

pub async fn test_write(d: &mut Box<dyn ConnTrait>) -> std::io::Result<()> {
    unsafe {
        d.write(&VEC2).await?;
    }

    Ok(())
}

static mut VEC1: [u8; 1024] = [0u8; 1024];
static mut VEC2: [u8; 1024] = [1u8; 1024];

#[tokio::test]
async fn te() {
    let _ = test_adder_r(10).await;
}
