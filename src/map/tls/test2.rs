use self::map::MapParams;

use std::path::PathBuf;

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
    layer_num: u8,
) -> anyhow::Result<Box<dyn ConnTrait>> {
    let cs = TcpStream::connect((listen_host_str, listen_port))
        .await
        .expect("dial tcp succeed");

    let a = super::client::Client::new("test.domain", true);
    let ta = net::Addr::from_strs("tcp", "", "1.2.3.4", 443)?; //not used in our test, but required by the method.

    let mut last_result: MapResult = MapResult::c(Box::new(cs));

    for _ in 0..layer_num {
        last_result = a
            .maps(
                CID::default(),
                ProxyBehavior::DECODE,
                MapParams {
                    c: Stream::Conn(last_result.c.try_unwrap_tcp().expect("last_result as c")),
                    a: Some(ta.clone()),
                    b: None,
                    d: Vec::new(),
                    shutdown_rx: None,
                },
            )
            .await;

        if let Some(e) = last_result.e {
            return Err(e);
        }
    }

    let r = last_result.c.try_unwrap_tcp().expect("last_result as c");

    Ok(r)
}

#[allow(unused)]
async fn listen_future(
    listen_host_str: &str,
    listen_port: u16,
    layer_num: u8,
) -> anyhow::Result<()> {
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

        let mut last_result: MapResult = MapResult::c(Box::new(nc));
        for _ in 0..layer_num {
            last_result = a
                .maps(
                    CID::default(),
                    ProxyBehavior::DECODE,
                    MapParams {
                        c: Stream::Conn(last_result.c.try_unwrap_tcp().expect("last_result as c")),
                        a: None,
                        b: None,
                        d: Vec::new(),
                        shutdown_rx: None,
                    },
                )
                .await;

            if let Some(e) = last_result.e {
                return Err(e);
            }
        }

        let conn = last_result.c;

        let mut c = conn.try_unwrap_tcp().expect("last_result as c");

        //let mut buf = BytesMut::zeroed(1024);
        loop {
            unsafe {
                c.read(&mut VEC1).await?;
            }
        }
        Ok::<(), anyhow::Error>(())
    });

    Ok(())
}

pub async fn test_init(layer_num: u8) -> anyhow::Result<Box<dyn ConnTrait>> {
    const HOST: &str = "127.0.0.1";
    const PORT: u16 = 23456;

    listen_future(HOST, PORT, layer_num).await?;

    dial_future(HOST, PORT, layer_num).await
}

pub async fn test_batch_run(l: usize, layer_num: u8) -> anyhow::Result<()> {
    let mut d = test_init(layer_num).await?;
    for _ in 0..l {
        test_write(&mut d).await?;
    }

    Ok(())
}

pub async fn test_write(d: &mut Box<dyn ConnTrait>) -> anyhow::Result<()> {
    unsafe {
        d.write(&VEC2).await?;
    }

    Ok(())
}

static mut VEC1: [u8; 1024] = [0u8; 1024];
static mut VEC2: [u8; 1024] = [1u8; 1024];

#[tokio::test]
async fn te() {
    let _ = test_batch_run(10, 2).await;
}
