use std::sync::Arc;

use bytes::{BufMut, BytesMut};
use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use crate::map::{MapParams, Mapper, ProxyBehavior, CID};
use crate::net::addr_conn::{AsyncReadAddrExt, AsyncWriteAddrExt};
use crate::net::{self, helpers, Addr};
use crate::user::AsyncUserAuthenticator;

use super::server::*;
use super::*;

#[test]
fn test224() {
    let str = sha224_hexstring_lower_case("pass");
    assert_eq!(str.len(), PASS_LEN);
    assert_eq!(
        str,
        "ccc9c73a37651c6b35de64c3a37858ccae045d285f57fffb409d251d"
    ); //valid string generated from another trojan implementation.
}

#[test]
fn test224_print() {
    let str = sha224_hexstring_lower_case("pass3");
    println!("{}", str)
}

async fn new_3user_trojan_inadder() -> Server {
    Server::new(Config {
        pass: Some("pass".to_string()),
        passes: Some(vec![
            "pass2".to_string(), //a2efc77b5d3c5e14ce7d0520115b32bba3426c1463d93d36a368fed7
            "pass3".to_string(), //aaae8f86690070b538d2fc141d6389dd9ce0e7d8e0a4d800384f9454
        ]),
    })
    .await
}

#[tokio::test]
async fn auth() -> std::io::Result<()> {
    let a = new_3user_trojan_inadder().await;
    assert!(
        a.um.auth_user_by_authstr(
            "trojan:ccc9c73a37651c6b35de64c3a37858ccae045d285f57fffb409d251d"
        )
        .await
        .unwrap()
        .plain_text_pass
            == "pass"
    );
    assert!(
        a.um.auth_user_by_authstr(
            "trojan:a2efc77b5d3c5e14ce7d0520115b32bba3426c1463d93d36a368fed7"
        )
        .await
        .unwrap()
        .plain_text_pass
            == "pass2"
    );
    assert!(
        a.um.auth_user_by_authstr(
            "trojan:aaae8f86690070b538d2fc141d6389dd9ce0e7d8e0a4d800384f9454"
        )
        .await
        .unwrap()
        .plain_text_pass
            == "pass3"
    );
    assert!(a
        .um
        .auth_user_by_authstr("trojan:aaae8f86690070b538d2fc141d6389dd9ce0e7d8e0a4d800384f9451")
        .await
        .is_none());
    Ok(())
}

#[tokio::test]
async fn auth_tcp_in_mem_earlydata() -> anyhow::Result<()> {
    let a = new_3user_trojan_inadder().await;
    let name = "www.b";
    let port: u16 = 43;
    let mut buf = BytesMut::with_capacity(100);
    let str = sha224_hexstring_lower_case("pass3");
    buf.put(str.as_bytes());
    buf.put_u16(CRLF);
    buf.put_u8(CMD_CONNECT);
    let addr = Addr::from_strs("tcp", name, "", port)?;
    net::helpers::addr_to_socks5_bytes(&addr, &mut buf);
    buf.put_u16(CRLF);
    buf.put(&b"hello!"[..]);

    println!("len is {}", buf.len());

    let writev = Arc::new(Mutex::new(Vec::new()));

    let client_tcps = net::helpers::MockTcpStream {
        read_data: buf.to_vec(),
        write_data: Vec::new(),
        write_target: Some(writev),
    };

    let r = a
        .maps(
            CID::default(),
            ProxyBehavior::DECODE,
            MapParams::new(Box::new(client_tcps)),
        )
        .await;
    match r.e {
        None => {
            let ad = r.a.unwrap();
            assert_eq!(ad.get_name().unwrap(), name);
            assert_eq!(ad.get_port(), port);
            assert_ne!(r.b, None);

            assert_eq!(&b"hello!"[..], r.b.unwrap());
        }
        Some(e) => {
            println!("{:?}", e);
            return Err(e);
        }
    }
    Ok(())
}

#[tokio::test]
async fn udp() -> anyhow::Result<()> {
    //写两遍，一遍错一遍对，然后在 另一端写一遍. same as test for socks5 udp

    let u = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ula = u.local_addr()?;
    println!("binded to , {}", ula);

    let so12345 = Addr::from_ip_addr_str("udp", "127.0.0.1:12345").unwrap();

    //let so12345c = so12345.clone();

    let (tx, mut rx) = mpsc::channel(10);

    tokio::spawn(async move {
        let (tcpc, _) = u.accept().await.unwrap();
        let mut buf = BytesMut::zeroed(1500);

        let mut ac = udp::from(Box::new(tcpc));

        for _ in 0..2 {
            println!("try read");

            let r = ac.r.read(&mut buf).await;
            println!("ok read");
            let x = tx.send(r).await;
            if x.is_err() {
                break;
            }
        }
        println!("try w2");

        let r =
            ac.w.write(b"dfg", &Addr::from_addr_str("udp", "5.6.7.8:90").unwrap())
                .await;

        println!("try w2 ok, {:?}", r);

        Ok::<(), anyhow::Error>(())
    });
    let mut nu = TcpStream::connect(ula).await?;

    println!("try send,  ",);
    nu.write(b"abc").await?;
    println!("ok send");

    let readr = rx.recv().await;

    println!("readr: {:#?}", readr);
    assert!(readr.unwrap().is_err());
    let mut buf = BytesMut::with_capacity(100);

    helpers::addr_to_socks5_bytes(&so12345, &mut buf);

    buf.put_u16(buf.len() as u16);
    buf.put_u16(CRLF);

    buf.extend_from_slice(b"abc");

    println!("try send2, {}", ula);
    nu.write(&buf).await?;
    println!("ok send2");

    let readr = rx.recv().await;

    println!("readr: {:?}", readr);
    assert!(readr.unwrap().is_ok());

    let n = nu.read(&mut buf).await?;

    buf.truncate(n);
    println!("rok, {n}");

    println!("rok, {:?}.  ", buf);

    Ok(())
}
