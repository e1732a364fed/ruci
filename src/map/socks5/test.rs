/*！
本测试文件中提供了大量纯手写的单元测试, 测试了各种可能的情况, 保证了 socks5 协议的实现的可靠性。

测试了 内存握手, 回环握手, 用户鉴权, ipv4,ipv6,domain, 和几种错误输入或攻击的情况

因为系统不会立即释放端口, 所以连续运行test有可能会报错, 手动运行是没问题的. 测试代码里面已经使用了随机端口。
*/

use bytes::{BufMut, BytesMut};
use parking_lot::Mutex;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;

use crate::map;
use crate::map::socks5::server::*;
use crate::map::MapParams;
use crate::map::Mapper;
use crate::map::ProxyBehavior;
use crate::map::CID;
use crate::net;
use crate::user::AsyncUserAuthenticator;
use crate::user::PlainText;
use futures::executor::block_on;
use futures::join;

use std::net::IpAddr;
use std::sync::Arc;

use crate::net::helpers::MockTcpStream;

use crate::map::socks5::{
    self, ATYP_DOMAIN, ATYP_IP4, ATYP_IP6, AUTH_NONE, AUTH_PASSWORD, CMD_CONNECT, VERSION5,
};

// let toml_str = r#"
//     protocol = "socks5"
//     uuid = "u0 p0"
//     users = [ { user = "u1", pass = "p1"},  { user = "u2", pass = "p2"}, ]
//     "#;
async fn new_3user_socks5_inadder() -> Server {
    Server::new(Config {
        support_udp: false,
        user_whitespace_pass: Some("u0 p0".to_string()),
        user_passes: Some(vec![PlainText::new("u1".to_string(), "p1".to_string())]),
    })
    .await
}

async fn new_noauth_socks5_inadder() -> Server {
    Server::new(Config::default()).await
}

/// 从toml配置创建socks5的adder后, 在内存中模拟连接 socks5 服务
#[tokio::test]
async fn auth_tcp_handshake_in_mem() -> anyhow::Result<()> {
    let a = new_3user_socks5_inadder().await;

    assert!(
        a.um.as_ref()
            .expect("a as um")
            .auth_user_by_authstr("plaintext:u1\np1")
            .await
            .expect("auth success")
            .pass
            == "p1"
    );

    assert!(
        a.um.as_ref()
            .expect("a as um")
            .auth_user_by_authstr("plaintext:u0\np0")
            .await
            .expect("auth success")
            .pass
            == "p0"
    );

    let writev = Arc::new(Mutex::new(Vec::new()));
    let writevc = writev.clone();

    let name = "www.b";
    let port: u16 = 43;
    let client_tcps = MockTcpStream {
        //userpass auth for u0, request for www.b:43
        read_data: vec![
            VERSION5,
            1,
            AUTH_PASSWORD,
            1,
            "u0".len() as u8,
            b'u',
            b'0',
            "p0".len() as u8,
            b'p',
            b'0',
            VERSION5,
            CMD_CONNECT,
            0,
            ATYP_DOMAIN,
            name.len() as u8,
            b'w',
            b'w',
            b'w',
            b'.',
            b'b',
            (port >> 8) as u8, /* 0 */
            port as u8,
        ],
        write_data: Vec::new(),
        write_target: Some(writev), //5 2 1 0 + ...
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
            let ad = r.a.expect("result.a has target_addr");
            assert_eq!(ad.get_name().expect("a has domain name"), name);
            assert_eq!(ad.get_port(), port);
            assert_eq!(r.b, None);

            let mut vf = Vec::from(&*socks5::COMMMON_TCP_HANDSHAKE_REPLY);
            let vhead = vec![VERSION5, AUTH_PASSWORD, 1, 0];
            vf.splice(0..0, vhead);
            println!("should be {:?}", vf);

            let v = writevc.lock();
            println!("it     be {:?}", v);

            assert!(v.eq(&vf));
        }
        Some(e) => {
            println!("{:?}", e);
            return Err(e);
        }
    }

    Ok(())
}

/// 从earlydata中读
#[tokio::test]
async fn auth_tcp_handshake_in_mem_earlydata() -> anyhow::Result<()> {
    let a = new_3user_socks5_inadder().await;

    let writev = Arc::new(Mutex::new(Vec::new()));
    let writevc = writev.clone();

    let name = "www.b";
    let port: u16 = 43;
    let client_tcps = MockTcpStream {
        //userpass auth for u0, request for www.b:43
        read_data: vec![],
        write_data: Vec::new(),
        write_target: Some(writev), //5 2 1 0 + ...
    };

    let ed: &[u8] = &[
        VERSION5,
        1,
        AUTH_PASSWORD,
        1,
        "u0".len() as u8,
        b'u',
        b'0',
        "p0".len() as u8,
        b'p',
        b'0',
        VERSION5,
        CMD_CONNECT,
        0,
        ATYP_DOMAIN,
        name.len() as u8,
        b'w',
        b'w',
        b'w',
        b'.',
        b'b',
        (port >> 8) as u8, /* 0 */
        port as u8,
    ];
    let earlybuf = BytesMut::from(ed);

    let r = a
        .maps(
            CID::default(),
            ProxyBehavior::DECODE,
            MapParams {
                c: map::Stream::Conn(Box::new(client_tcps)),
                a: None,
                b: Some(earlybuf),
                d: Vec::new(),
                shutdown_rx: None,
            },
        )
        .await;
    match r.e {
        None => {
            let ad = r.a.expect("result.a has value");
            assert_eq!(ad.get_name().expect("target_addr is name"), name);
            assert_eq!(ad.get_port(), port);
            assert_eq!(r.b, None);

            let mut vf = Vec::from(&*socks5::COMMMON_TCP_HANDSHAKE_REPLY);
            let vhead = vec![VERSION5, AUTH_PASSWORD, 1, 0];
            vf.splice(0..0, vhead);
            println!("should be {:?}", vf);

            let v = writevc.lock();
            println!("it     be {:?}", v);

            assert!(v.eq(&vf));
        }
        Some(e) => {
            println!("{:?}", e);
            return Err(e);
        }
    }

    Ok(())
}

#[tokio::test]
async fn auth_tcp_handshake_local() -> anyhow::Result<()> {
    let ps = net::gen_random_higher_port();

    let a = new_3user_socks5_inadder().await;
    let listen_host = "127.0.0.1".to_string();
    let listen_port = ps;

    let listener = TcpListener::bind(listen_host.clone() + ":" + &listen_port.to_string())
        .await
        .expect("listen successful");

    let target_name = "www.b";
    let target_port: u16 = 43;

    let listen_future = async {
        let r = listener.accept().await;
        let (ss, _) = r.expect("listener.accept returns a valid stream");
        // let (mut newc, addr, client_data, authed_user) =
        //     a.add(1, Box::new(ss), None, None).await.unwrap();

        let r = a
            .maps(
                CID::default(),
                ProxyBehavior::DECODE,
                MapParams::new(Box::new(ss)),
            )
            .await;

        let ad = r.a.expect("result.a has value");
        assert_eq!(ad.get_name().expect("a is name"), target_name);
        assert_eq!(ad.get_port(), target_port);
        assert_eq!(r.b, None);

        match r.d {
            Some(d) => {
                if let Some(up) = d.get_user() {
                    assert_eq!(up.identity_str(), "u0");
                    assert_eq!(up.auth_str(), "plaintext:u0\np0");
                    println!("auth user succeed")
                } else {
                    panic!("socks5 should returns a User data")
                }
            }
            None => panic!("got None instead of   data"),
        }

        //接收测试数据
        let mut readbuf = [0u8; 1024];
        let n = r.c.try_unwrap_tcp()?.read(&mut readbuf[..]).await?;
        assert_eq!(&readbuf[..n], &[b'h', b'e', b'l', b'l', b'o']);

        Ok::<(), anyhow::Error>(())
    };

    let dial_future = async {
        let mut cs = TcpStream::connect((listen_host.as_str(), listen_port))
            .await
            .expect("dial ok");

        let mut readbuf = [0u8; 1024];

        cs.write(&[VERSION5, 1, AUTH_PASSWORD])
            .await
            .expect("write1 ok");

        //tokio::time::sleep(time::Duration::from_secs(1));

        // 如果不read,  server会在 add方法中的 写回 auth 成功的reply 时报错：
        // An established connection was aborted by the software in your host machine.

        let n = cs.read(&mut readbuf[..]).await.expect("read1 ok");
        println!("client read, {:?}", &readbuf[..n]);

        assert_eq!(&readbuf[..n], &[5, 2]);

        cs.write(&[
            1,
            "u0".len() as u8,
            b'u',
            b'0',
            "p0".len() as u8,
            b'p',
            b'0',
        ])
        .await
        .expect("write ok");

        //tokio::time::sleep(time::Duration::from_secs(1));

        let n = cs.read(&mut readbuf[..]).await.expect("read2 ok");
        println!("client read, {:?}", &readbuf[..n]);
        assert_eq!(&readbuf[..n], &[1, 0]);

        cs.write(&[
            VERSION5,
            CMD_CONNECT,
            0,
            ATYP_DOMAIN,
            target_name.len() as u8,
            b'w',
            b'w',
            b'w',
            b'.',
            b'b',
            (target_port >> 8) as u8,
            target_port as u8,
        ])
        .await
        .expect("write2 ok");

        let n = cs.read(&mut readbuf[..]).await.expect("read3 ok");
        println!("client read, {:?}", &readbuf[..n]);
        assert_eq!(&readbuf[..n], &*socks5::COMMMON_TCP_HANDSHAKE_REPLY);

        //发送测试数据
        cs.write(&b"hello"[..]).await.expect("write3 ok");
        1
    };

    let _ = join!(listen_future, dial_future);

    Ok(())
}

#[tokio::test]
async fn auth_tcp_handshake_local_with_ip4_request_and_bytes_crate() -> anyhow::Result<()> {
    let ps = net::gen_random_higher_port();

    let a = new_3user_socks5_inadder().await;

    let listen_host = "127.0.0.1".to_string();
    let listen_port = ps;

    let listener = TcpListener::bind(listen_host.clone() + ":" + &listen_port.to_string())
        .await
        .unwrap();

    let target_name = "123.123.123.123";
    let target_port: u16 = 80;

    let listen_future = async {
        let r = listener.accept().await;
        let (ss, _) = r.unwrap();
        //let (mut newc, addr, client_data, _) = a.add(1, Box::new(ss), None, None).await.unwrap();

        let r = a
            .maps(
                CID::default(),
                ProxyBehavior::DECODE,
                MapParams::new(Box::new(ss)),
            )
            .await;

        let ad = r.a.unwrap();
        assert_eq!(ad.get_ip().unwrap(), target_name.parse::<IpAddr>().unwrap());
        assert_eq!(ad.get_port(), target_port);
        assert_eq!(r.b, None);

        //接收测试数据
        let mut readbuf = [0u8; 1024];
        let n = r.c.try_unwrap_tcp()?.read(&mut readbuf[..]).await.unwrap();
        assert_eq!(&readbuf[..n], &b"hello"[..]);

        Ok::<(), anyhow::Error>(())
    };

    let dial_future = async {
        let mut cs = TcpStream::connect((listen_host.as_str(), listen_port))
            .await
            .unwrap();

        let mut readbuf = [0u8; 1024];

        cs.write(&[VERSION5, 1, AUTH_PASSWORD]).await.unwrap();

        //tokio::time::sleep(time::Duration::from_secs(1));

        // 如果不read,  server会在 add方法中的 写回 auth 成功的reply 时报错：
        // An established connection was aborted by the software in your host machine.

        let n = cs.read(&mut readbuf[..]).await.unwrap();
        println!("client read, {:?}", &readbuf[..n]);

        assert_eq!(&readbuf[..n], &[5, 2]);

        cs.write(&[
            1,
            "u0".len() as u8,
            b'u',
            b'0',
            "p0".len() as u8,
            b'p',
            b'0',
        ])
        .await
        .unwrap();

        //tokio::time::sleep(time::Duration::from_secs(1));

        let n = cs.read(&mut readbuf[..]).await.unwrap();
        println!("client read, {:?}", &readbuf[..n]);
        assert_eq!(&readbuf[..n], &[1, 0]);

        let mut writebuf = BytesMut::with_capacity(1024);

        writebuf.truncate(0);
        writebuf.put(&[VERSION5, CMD_CONNECT, 0, ATYP_IP4][..]);

        let ipa = target_name.parse::<IpAddr>().unwrap();
        let vu = net::ip_addr_to_u8_vec(ipa);

        writebuf.put(vu.as_slice());

        writebuf.put(&[(target_port >> 8) as u8, target_port as u8][..]);

        cs.write(&writebuf).await.unwrap();

        let n = cs.read(&mut readbuf[..]).await.unwrap();
        println!("client read, {:?}", &readbuf[..n]);
        assert_eq!(&readbuf[..n], &*socks5::COMMMON_TCP_HANDSHAKE_REPLY);

        //发送测试数据
        cs.write(&b"hello"[..]).await.unwrap();
        1
    };

    let _ = join!(listen_future, dial_future);

    Ok(())
}

#[tokio::test]
async fn auth_tcp_handshake_local_with_ip6_request_and_bytes_crate() -> anyhow::Result<()> {
    let ps = net::gen_random_higher_port();

    let a = new_3user_socks5_inadder().await;

    let listen_host = "127.0.0.1".to_string();
    let listen_port = ps;

    let listener = TcpListener::bind(listen_host.clone() + ":" + &listen_port.to_string())
        .await
        .unwrap();

    let target_name = net::gen_random_ipv6().to_string();
    let target_port: u16 = 80;

    let listen_future = async {
        let r = listener.accept().await;
        let (ss, _) = r.unwrap();
        let r = a
            .maps(
                CID::default(),
                ProxyBehavior::DECODE,
                MapParams::new(Box::new(ss)),
            )
            .await;

        let ad = r.a.unwrap();
        assert_eq!(ad.get_ip().unwrap(), target_name.parse::<IpAddr>().unwrap());
        assert_eq!(ad.get_port(), target_port);
        assert_eq!(r.b, None);

        //接收测试数据
        let mut readbuf = [0u8; 1024];
        let n = r.c.try_unwrap_tcp()?.read(&mut readbuf[..]).await.unwrap();
        assert_eq!(&readbuf[..n], &b"hello"[..]);

        Ok::<_, anyhow::Error>(())
    };

    let dial_future = async {
        let mut cs = TcpStream::connect((listen_host.as_str(), listen_port))
            .await
            .unwrap();

        let mut readbuf = [0u8; 1024];

        cs.write(&[VERSION5, 1, AUTH_PASSWORD]).await.unwrap();

        //tokio::time::sleep(time::Duration::from_secs(1));

        // 如果不read,  server会在 add方法中的 写回 auth 成功的reply 时报错：
        // An established connection was aborted by the software in your host machine.

        let n = cs.read(&mut readbuf[..]).await.unwrap();
        println!("client read, {:?}", &readbuf[..n]);

        assert_eq!(&readbuf[..n], &[5, 2]);

        cs.write(&[
            1,
            "u0".len() as u8,
            b'u',
            b'0',
            "p0".len() as u8,
            b'p',
            b'0',
        ])
        .await
        .unwrap();

        //tokio::time::sleep(time::Duration::from_secs(1));

        let n = cs.read(&mut readbuf[..]).await.unwrap();
        println!("client read, {:?}", &readbuf[..n]);
        assert_eq!(&readbuf[..n], &[1, 0]);

        let mut writebuf = BytesMut::with_capacity(1024);

        writebuf.truncate(0);
        writebuf.put(&[VERSION5, CMD_CONNECT, 0, ATYP_IP6][..]);

        let ipa = target_name.parse::<IpAddr>().unwrap();
        let vu = net::ip_addr_to_u8_vec(ipa);

        writebuf.put(vu.as_slice());

        writebuf.put(&[(target_port >> 8) as u8, target_port as u8][..]);

        cs.write(&writebuf).await.unwrap();

        let n = cs.read(&mut readbuf[..]).await.unwrap();
        println!("client read, {:?}", &readbuf[..n]);
        assert_eq!(&readbuf[..n], &*socks5::COMMMON_TCP_HANDSHAKE_REPLY);

        //发送测试数据
        cs.write(&b"hello"[..]).await.unwrap();
        1
    };

    let _ = join!(listen_future, dial_future);

    Ok(())
}

#[tokio::test]
async fn no_auth_tcp_handshake_in_mem() -> anyhow::Result<()> {
    let a = new_noauth_socks5_inadder().await;

    //因为我们无法再从 client_tcps 中取出数据了, 因为它放到Box后就属于Addr了
    // 所以要用Arc<Mutex<>> 结构
    // 和 clone , 从clone的 指针中获取数据。

    let writev = Arc::new(Mutex::new(Vec::new()));
    let writevc = writev.clone();

    let name = "www.b";
    let port = 65500;
    let client_tcps = MockTcpStream {
        //no auth, request for www.b:65500
        read_data: vec![
            VERSION5,
            1,
            AUTH_NONE,
            VERSION5,
            CMD_CONNECT,
            0,
            ATYP_DOMAIN,
            name.len() as u8,
            b'w',
            b'w',
            b'w',
            b'.',
            b'b',
            (port >> 8) as u8,
            port as u8,
        ],
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
            assert!(r.a.unwrap().get_name().unwrap() == name);
            assert!(r.b == None);

            //收到的应为两个 reply 相加, 第一个为5 0, 第二个为 COMMMON_TCP_HANDSHAKE_REPLY

            let mut vf = Vec::from(&*socks5::COMMMON_TCP_HANDSHAKE_REPLY);
            let vhead = vec![VERSION5, AUTH_NONE];
            vf.splice(0..0, vhead);

            println!("should be {:?}", vf);

            let v = writevc.lock();
            println!("it     be {:?}", v);

            assert!(v.eq(&vf));
        }
        Some(e) => {
            println!("{:?}", e);
            return Err(e);
        }
    }

    Ok(())
}

/// 在握手数据后连上一个客户数据hello一起发送(earlydata)
#[tokio::test]
async fn no_auth_tcp_handshake_in_mem_stick_hello() -> anyhow::Result<()> {
    let a = new_noauth_socks5_inadder().await;
    let writev = Arc::new(Mutex::new(Vec::new()));
    let writevc = writev.clone();

    let name = "www.b";
    let port = 65500;
    let client_tcps = MockTcpStream {
        //no auth, request for www.b:65500
        read_data: vec![
            VERSION5,
            1,
            AUTH_NONE,
            VERSION5,
            CMD_CONNECT,
            0,
            ATYP_DOMAIN,
            name.len() as u8,
            b'w',
            b'w',
            b'w',
            b'.',
            b'b',
            (port >> 8) as u8,
            port as u8,
            b'h',
            b'e',
            b'l',
            b'l',
            b'o',
        ],
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
            assert_eq!(r.a.unwrap().get_name().unwrap(), name);
            assert_eq!(r.b.unwrap(), b"hello"[..]);

            let mut vf = Vec::from(&*socks5::COMMMON_TCP_HANDSHAKE_REPLY);
            let vhead = vec![VERSION5, AUTH_NONE];
            vf.splice(0..0, vhead);

            println!("should be {:?}", vf);

            let v = writevc.lock();
            println!("it     be {:?}", v);

            assert!(v.eq(&vf));
        }
        Some(e) => {
            println!("{:?}", e);
            return Err(e);
        }
    }

    Ok(())
}

#[tokio::test]
#[should_panic]
async fn wrong0_no_auth_tcp_handshake_in_mem() {
    //在下面客户端write的数据中, version不为5, server理应返回error
    std::env::set_var("RUST_BACKTRACE", "0");

    let a = new_noauth_socks5_inadder().await;
    const WRONG_V: u8 = 8;
    let name = "www.b";
    let client_tcps = MockTcpStream {
        //no auth, request for www.b
        read_data: vec![
            WRONG_V,
            1,
            AUTH_NONE,
            WRONG_V,
            CMD_CONNECT,
            0,
            ATYP_DOMAIN,
            name.len() as u8,
            b'w',
            b'w',
            b'w',
            b'.',
            b'b',
        ],
        write_data: Vec::new(),
        write_target: None,
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
            assert!(r.a.unwrap().get_name().unwrap() == name);
        }
        Some(e) => {
            panic!("{:?}", e);
        }
    }
}

#[tokio::test]
#[should_panic]
async fn wrong1_no_auth_tcp_handshake_in_mem() {
    //在下面客户端write的数据中, 没有给出port, 服务端理应返回error
    std::env::set_var("RUST_BACKTRACE", "0");

    let a = new_noauth_socks5_inadder().await;

    let name = "www.b";
    let client_tcps = MockTcpStream {
        //no auth, request for www.b
        read_data: vec![
            VERSION5,
            1,
            AUTH_NONE,
            VERSION5,
            CMD_CONNECT,
            0,
            ATYP_DOMAIN,
            name.len() as u8,
            b'w',
            b'w',
            b'w',
            b'.',
            b'b',
        ],
        write_data: Vec::new(),
        write_target: None,
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
            assert!(r.a.unwrap().get_name().unwrap() == name);
        }
        Some(e) => {
            panic!("{:?}", e);
        }
    }
}

#[tokio::test]
async fn batch_random_bytes_request_no_auth_tcp_handshake_in_mem() -> anyhow::Result<()> {
    std::env::set_var("RUST_BACKTRACE", "0");
    for n in 0..1000 {
        let result = std::panic::catch_unwind(|| {
            let f = random_bytes_request_no_auth_tcp_handshake_in_mem();
            let r = block_on(f);
            if r.is_err() {
                panic!("panic on err!, {}", n);
            }
        });

        if !result.is_err() {
            panic!("No panic was caught!, {}", n);
        }
    }

    Ok(())
}

async fn random_bytes_request_no_auth_tcp_handshake_in_mem() -> anyhow::Result<()> {
    //在下面客户端write的数据中, 使用随机字节发送给服务端, 服务端理应返回error
    //不过第一位设为5, 因为我们已知第一位不为5时肯定报错了(在 wrong0_no_auth_tcp_handshake_in_mem 中测了)

    let a = new_noauth_socks5_inadder().await;

    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};
    let mut rng = SmallRng::from_entropy();
    let mut random_bytes: Vec<u8> = (0..80).map(|_| rng.gen::<u8>()).collect();
    random_bytes[0] = VERSION5;
    let client_tcps = MockTcpStream {
        read_data: random_bytes,
        write_data: Vec::new(),
        write_target: None,
    };

    let r = a
        .maps(
            CID::default(),
            ProxyBehavior::DECODE,
            MapParams::new(Box::new(client_tcps)),
        )
        .await;
    match r.e {
        None => {}

        Some(e) => {
            println!("{:?}", e);
            return Err(e);
        }
    }

    Ok(())
}

#[tokio::test]
async fn batch_random_bytes_request_auth_userpass_tcp_handshake_in_mem() -> anyhow::Result<()> {
    std::env::set_var("RUST_BACKTRACE", "0");
    for n in 0..1000 {
        let result = std::panic::catch_unwind(|| {
            let f = random_bytes_request_auth_userpass_tcp_handshake_in_mem();
            let r = block_on(f);
            if r.is_err() {
                panic!("panic on err!, {}", n);
            }
        });

        if !result.is_err() {
            panic!("No panic was caught!, {}", n);
        }
    }

    Ok(())
}

async fn random_bytes_request_auth_userpass_tcp_handshake_in_mem() -> anyhow::Result<()> {
    //在下面客户端write的数据中, 使用随机字节发送给服务端, 服务端理应返回error
    //不过第一位设为5, 因为我们已知第一位不为5时肯定报错了

    let a = new_3user_socks5_inadder().await;

    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};
    let mut rng = SmallRng::from_entropy();
    let mut random_bytes: Vec<u8> = (0..80).map(|_| rng.gen::<u8>()).collect();
    random_bytes[0] = VERSION5;
    let client_tcps = MockTcpStream {
        read_data: random_bytes,
        write_data: Vec::new(),
        write_target: None,
    };

    let r = a
        .maps(
            CID::default(),
            ProxyBehavior::DECODE,
            MapParams::new(Box::new(client_tcps)),
        )
        .await;
    match r.e {
        None => {}
        Some(e) => {
            println!("{:?}", e);
            return Err(e);
        }
    }

    Ok(())
}
