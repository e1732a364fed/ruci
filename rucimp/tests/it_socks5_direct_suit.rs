/*!
 * 集成测试 socks5 -> direct 的情况, 采用了随机端口，以百度为目标网页
测试了 earlydata 的情况
测试了 同时监听三个端口 的情况
测试了 不监视流量 的情况 （no_transmission_info)
测试了 监视实际流量的情况 (counter)
测试了 直接调用 rucimp::listen_ser 和 用 rucimp::SuitEngine 的情况
测试了 SuitEngine 的 block_run 和 run 两种情况
测试了 同时发起两个请求的情况 (异步)
测试了 发起长时间挂起的请求的情况 （非法）
 */

use std::{env::set_var, io, sync::Arc, time::Duration};

use async_std::{
    io::{ReadExt, WriteExt},
    net::TcpStream,
    task,
};
use async_std_test::async_test;
use bytes::{BufMut, BytesMut};
use futures::{pin_mut, select, FutureExt};
use log::info;
use ruci::map::{self, socks5::*};
use ruci::map::{socks5, MappersVec};
use ruci::{map::Mapper, net, user::UserPass};
use rucimp::suit::config::Config;
use rucimp::{load_in_adder_by_str, load_out_adder_by_str, suit::*, SuitEngine};

const WAITSECS: u64 = ruci::relay::READ_HANDSHAKE_TIMEOUT + 2;
const WAITID: i32 = 10101;

const TARGET_PORT: u16 = 80;
const TARGET_NAME: &str = "www.baidu.com";

//参见 src/socks5/test.rs
async fn f_dial_future(
    rid: i32,
    listen_host_str: &str,
    listen_port: u16,
    the_target_name: &str,
    the_target_port: u16,
) {
    info!("start run f_dial_future, {}", rid);
    let mut cs = TcpStream::connect((listen_host_str, listen_port))
        .await
        .unwrap();

    let mut readbuf = [0u8; 1024];

    if rid == WAITID {
        task::sleep(Duration::from_secs(WAITSECS)).await;
    }

    cs.write(&[VERSION5, 1, AUTH_PASSWORD]).await.unwrap();

    let n = cs.read(&mut readbuf[..]).await.unwrap();
    info!("client{} read, {:?}", rid, &readbuf[..n]);

    assert_eq!(&readbuf[..n], &[5, 2]);
    if rid == WAITID {
        task::sleep(Duration::from_secs(WAITSECS)).await;
    }
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

    let n = cs.read(&mut readbuf[..]).await.unwrap();
    info!("client{} read, {:?}", rid, &readbuf[..n]);
    assert_eq!(&readbuf[..n], &[1, 0]);

    let mut buf = BytesMut::with_capacity(1024);
    buf.put(
        &[
            VERSION5,
            CMD_CONNECT,
            0,
            ATYP_DOMAIN,
            the_target_name.len() as u8,
        ][..],
    );

    buf.put(the_target_name.as_bytes());

    buf.put(&[(the_target_port >> 8) as u8, the_target_port as u8][..]);
    if rid == WAITID {
        task::sleep(Duration::from_secs(WAITSECS)).await;
    }
    cs.write(&buf).await.unwrap();

    let n = cs.read(&mut readbuf[..]).await.unwrap();
    info!("client{} read, {:?}", rid, &readbuf[..n]);
    assert_eq!(&readbuf[..n], &*socks5::COMMMON_TCP_HANDSHAKE_REPLY);

    info!("client{} writing hello...", rid,);
    //发送测试数据
    if rid == WAITID {
        task::sleep(Duration::from_secs(WAITSECS)).await;
    }
    cs.write(&b"hello\n"[..]).await.unwrap();

    info!("client{} reading...", rid,);

    let n = cs.read(&mut readbuf[..]).await.unwrap();
    info!("client{} read, {:?}", rid, &readbuf[..n]);
    info!(
        "client{} read str is, {:?}",
        rid,
        String::from_utf8_lossy(&readbuf[..n])
    );

    // info!("client will close...",);

    // let _ =  cs.shutdown(std::net::Shutdown::Both);
    // info!("client closed");
}

async fn f_dial_future_out_adder(
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

    if rid == WAITID {
        task::sleep(Duration::from_secs(WAITSECS)).await;
    }

    let a = socks5::client::Client {
        up: Some(UserPass::from("u0 p0".to_string())),
        use_earlydata: false,
    };
    let mut newconn = a
        .maps(
            0,
            ruci::map::ProxyBehavior::ENCODE,
            ruci::map::MapParams {
                c: map::Stream::TCP(Box::new(cs)),
                a: Some(net::Addr::from_strs(
                    "tcp",
                    the_target_name,
                    "",
                    the_target_port,
                )?),
                b: None,
                d: None,
            },
        )
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

async fn f_dial_future_earlydata(
    rid: i32,
    listen_host_str: &str,
    listen_port: u16,
    the_target_name: &str,
    the_target_port: u16,
) {
    info!("start run f_dial_future, {}", rid);
    let mut cs = TcpStream::connect((listen_host_str, listen_port))
        .await
        .unwrap();

    let mut readbuf = [0u8; 1024];

    // if rid == WAITID {
    //     task::sleep(Duration::from_secs(WAITSECS)).await;
    // }

    cs.write(&[VERSION5, 1, AUTH_PASSWORD]).await.unwrap();

    let n = cs.read(&mut readbuf[..]).await.unwrap();
    info!("client{} read, {:?}", rid, &readbuf[..n]);

    assert_eq!(&readbuf[..n], &[5, 2]);
    // if rid == WAITID {
    //     task::sleep(Duration::from_secs(WAITSECS)).await;
    // }
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

    let n = cs.read(&mut readbuf[..]).await.unwrap();
    info!("client{} read, {:?}", rid, &readbuf[..n]);
    assert_eq!(&readbuf[..n], &[1, 0]);

    let mut buf = BytesMut::with_capacity(1024);
    buf.put(
        &[
            VERSION5,
            CMD_CONNECT,
            0,
            ATYP_DOMAIN,
            the_target_name.len() as u8,
        ][..],
    );

    buf.put(the_target_name.as_bytes());

    buf.put(&[(the_target_port >> 8) as u8, the_target_port as u8][..]);

    buf.put(&b"hello\n"[..]);

    // if rid == WAITID {
    //     task::sleep(Duration::from_secs(WAITSECS)).await;
    // }
    cs.write(&buf).await.unwrap();

    let n = cs.read(&mut readbuf[..]).await.unwrap();
    info!("client{} read, {:?}", rid, &readbuf[..n]);
    assert_eq!(&readbuf[..n], &*socks5::COMMMON_TCP_HANDSHAKE_REPLY);

    //info!("client{} writing hello...", rid,);
    //发送测试数据
    // if rid == WAITID {
    //     task::sleep(Duration::from_secs(WAITSECS)).await;
    // }
    // cs.write(&b"hello\n"[..]).await.unwrap();

    info!("client{} reading...", rid,);

    let n = cs.read(&mut readbuf[..]).await.unwrap();
    info!("client{} read, {:?}", rid, &readbuf[..n]);
    info!(
        "client{} read str is, {:?}",
        rid,
        String::from_utf8_lossy(&readbuf[..n])
    );

    // info!("client will close...",);

    // let _ =  cs.shutdown(std::net::Shutdown::Both);
    // info!("client closed");
}

fn get_config_str() -> String {
    let toml_str = r#"
    [[listen]]
    protocol = "socks5"
    host = "127.0.0.1"
    port = 12345
    uuid = "u0 p0"
    users = [ { user = "u1", pass = "p1"},  { user = "u2", pass = "p2"}, ]

    [[dial]]
    protocol = "direct"
    "#;

    let ps = net::gen_random_higher_port().to_string();
    let toml_str = toml_str.replace("12345", &ps);

    return toml_str;
}

fn get_config() -> Config {
    get_nl_config(1)
}

fn get_nl_config(listener_num: u8) -> Config {
    let mut ws = String::new();
    for _ in 0..listener_num {
        let toml_str = get_config_str();

        ws += &toml_str;
    }
    let c: Config = toml::from_str(&ws).unwrap();
    //println!("{:#?}", c);

    c
}

async fn get_socks5_inadder(lsuit: &SuitStruct) -> socks5::server::Server {
    let a = socks5::server::Server::new(
        rucimp::suit::config::adapter::get_socks5_server_option_from_ldconfig(
            lsuit.get_config().unwrap().clone(),
        ),
    )
    .await;
    a
}

/// 基本测试. 百度在遇到非http请求后会主动断开连接，其对于长挂起请求最多60秒后断开连接。
/// 其对请求中不含\n 时会视为挂起
#[async_test]
async fn socks5_direct_and_request() -> std::io::Result<()> {
    set_var("RUST_LOG", "debug");
    let _ = env_logger::try_init();

    info!("start socks5_direct_and_request_baidu test");

    let mut c = get_config();

    let mut lsuit = SuitStruct::from(c.listen.pop().unwrap());
    lsuit.set_behavior(ruci::map::ProxyBehavior::DECODE);

    let a = get_socks5_inadder(&lsuit).await;
    lsuit.push_mapper(Box::new(a));

    //println!("{:?}", lsuit);

    let csuit = SuitStruct::from(c.dial.pop().unwrap());
    //println!("{:?}", csuit);
    let ti = net::TransmissionInfo::default();
    let arc_ti = Arc::new(ti);
    let arc_tic = arc_ti.clone();

    let alsuit = Arc::new(lsuit);
    let alsuitc = alsuit.clone();

    let listen_future = async {
        info!("try start listen");

        let r = listen_ser(alsuit, Arc::new(csuit), Some(arc_ti)).await;

        info!("r {:?}", r);
    };

    let cc = alsuitc.get_config().unwrap().clone();
    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future = f_dial_future(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    pin_mut!(listen_future, dial_future);

    /*

    baidu应返回 ：HTTP/1.1 400 Bad Request\r\n\r\n

    copy 中的 half2 会先返回，即从 到百度的连接 到 到socks5的连接 的拷贝 会断开（到百度的连接 是 被百度自动断开的）

     */

    select! {
        () = listen_future => {
            panic!("listen finished first");
        },
        () = dial_future => {
            info!("dial finished first , {:?}",arc_tic);

        },
    }

    Ok(())
}

#[async_test]
async fn socks5_direct_and_outadder() -> std::io::Result<()> {
    set_var("RUST_LOG", "debug");
    let _ = env_logger::try_init();

    info!("start socks5_direct_and_request_baidu test");

    let mut c = get_config();

    let mut lsuit = SuitStruct::from(c.listen.pop().unwrap());
    lsuit.set_behavior(ruci::map::ProxyBehavior::DECODE);

    let a = get_socks5_inadder(&lsuit).await;
    lsuit.push_mapper(Box::new(a));

    let csuit = SuitStruct::from(c.dial.pop().unwrap());
    let ti = net::TransmissionInfo::default();
    let arc_ti = Arc::new(ti);

    let alsuit = Arc::new(lsuit);
    let alsuitc = alsuit.clone();

    let listen_future = async {
        info!("try start listen");

        let r = listen_ser(alsuit, Arc::new(csuit), Some(arc_ti)).await;

        info!("r {:?}", r);
    };

    let cc = alsuitc.get_config().unwrap().clone();
    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future =
        f_dial_future_out_adder(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    pin_mut!(listen_future, dial_future);

    select! {
        () = listen_future => {
            panic!("listen finished first");
        },
        r = dial_future => {
            info!("dial finished first , {:?}",r);

        },
    }
    Ok(())
}

/// 不监视原始流量，性能会高一些
#[async_test]
async fn socks5_direct_and_request_no_transmission_info() -> std::io::Result<()> {
    set_var("RUST_LOG", "debug");
    let _ = env_logger::try_init();

    info!("start socks5_direct_and_request_baidu test");

    let mut c = get_config();

    let mut lsuit = SuitStruct::from(c.listen.pop().unwrap());
    lsuit.set_behavior(ruci::map::ProxyBehavior::DECODE);

    let a = get_socks5_inadder(&lsuit).await;
    lsuit.push_mapper(Box::new(a));

    let csuit = SuitStruct::from(c.dial.pop().unwrap());

    let alsuit = Arc::new(lsuit);
    let alsuitc = alsuit.clone();

    let listen_future = async {
        info!("try start listen");

        let r = listen_ser(alsuit, Arc::new(csuit), None).await;

        info!("r {:?}", r);
    };

    let cc = alsuitc.get_config().unwrap().clone();
    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future = f_dial_future(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    pin_mut!(listen_future, dial_future);

    select! {
        () = listen_future => {
            panic!("listen finished first");
        },
        () = dial_future => {
            info!("dial finished first ", );

        },
    }

    Ok(())
}

/// 不监视原始流量但是监视实际流量，用 加一层 InCounter 实现。
///
/// 注：这里就体现了链式代理的特点。可以里一层counter 外一层counter 如 counter - socks5 - counter 来分别记录原始流量
/// 和实际流量
#[async_test]
async fn socks5_direct_and_request_counter() -> std::io::Result<()> {
    set_var("RUST_LOG", "debug");
    let _ = env_logger::try_init();

    info!("start socks5_direct_and_request_baidu test");

    let mut c = get_config();

    let mut lsuit = SuitStruct::from(c.listen.pop().unwrap());
    lsuit.set_behavior(ruci::map::ProxyBehavior::DECODE);

    let a = ruci::map::counter::Counter;
    lsuit.push_mapper(Box::new(a));

    let a = get_socks5_inadder(&lsuit).await;
    lsuit.push_mapper(Box::new(a));

    let wn = lsuit.whole_name.clone();

    let csuit = SuitStruct::from(c.dial.pop().unwrap());

    let alsuit = Arc::new(lsuit);
    let alsuitc = alsuit.clone();

    let listen_future = async {
        info!("try start listen, {}", wn);

        let r = listen_ser(alsuit, Arc::new(csuit), None).await;

        info!("r {:?}", r);
    };

    let cc = alsuitc.get_config().unwrap().clone();
    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future = f_dial_future(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    pin_mut!(listen_future, dial_future);

    select! {
        () = listen_future => {
            panic!("listen finished first");
        },
        () = dial_future => {
            info!("dial finished first ", );

        },
    }

    Ok(())
}

#[async_test]
async fn socks5_direct_and_request_earlydata() -> std::io::Result<()> {
    set_var("RUST_LOG", "debug");
    let _ = env_logger::try_init();

    info!("start socks5_direct_and_request_baidu test");

    let mut c = get_config();

    let mut lsuit = SuitStruct::from(c.listen.pop().unwrap());
    lsuit.set_behavior(ruci::map::ProxyBehavior::DECODE);

    let a = get_socks5_inadder(&lsuit).await;
    lsuit.push_mapper(Box::new(a));

    let csuit = SuitStruct::from(c.dial.pop().unwrap());
    let ti = net::TransmissionInfo::default();
    let arc_ti = Arc::new(ti);
    let arc_tic = arc_ti.clone();

    let alsuit = Arc::new(lsuit);
    let alsuitc = alsuit.clone();

    let listen_future = async {
        info!("try start listen");

        let r = listen_ser(alsuit, Arc::new(csuit), Some(arc_ti)).await;

        info!("r {:?}", r);
    };

    let cc = alsuitc.get_config().unwrap().clone();
    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future =
        f_dial_future_earlydata(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    pin_mut!(listen_future, dial_future);

    select! {
        () = listen_future => {
            panic!("listen finished first");
        },
        () = dial_future => {
            info!("dial finished first , {:?}",arc_tic);

        },
    }

    Ok(())
}

//因为太耗时，所以test被注释掉
/// 每次write前等待 ruci::proxy::READ_HANDSHAKE_TIMEOUT + 2 秒
#[async_test]
#[should_panic]
#[allow(dead_code)]
async fn socks5_direct_longwait_write_and_request() -> std::io::Result<()> {
    set_var("RUST_LOG", "debug");
    let _ = env_logger::try_init();
    set_var("RUST_BACKTRACE", "0");

    info!("start socks5_direct_and_request_baidu test");

    let mut c = get_config();

    let mut lsuit = SuitStruct::from(c.listen.pop().unwrap());
    lsuit.set_behavior(ruci::map::ProxyBehavior::DECODE);

    let a = get_socks5_inadder(&lsuit).await;
    lsuit.push_mapper(Box::new(a));

    let csuit = SuitStruct::from(c.dial.pop().unwrap());
    let ti = net::TransmissionInfo::default();
    let arc_ti = Arc::new(ti);
    let arc_tic = arc_ti.clone();

    let alsuit = Arc::new(lsuit);
    let alsuitc = alsuit.clone();

    let listen_future = async {
        info!("try start listen");

        let r = listen_ser(alsuit, Arc::new(csuit), Some(arc_ti)).await;

        info!("r {:?}", r);
    };

    let cc = alsuitc.get_config().unwrap().clone();
    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future =
        f_dial_future(WAITID, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    pin_mut!(listen_future, dial_future);

    select! {
        () = listen_future => {
            panic!("listen finished first");
        },
        () = dial_future => {
            info!("dial finished first, will return in 2 secs... , {:?}",arc_tic);

        },
    }

    task::sleep(Duration::from_secs(2)).await;

    Ok(())
}

/// 对 block_run 和 non_block run 各测一次
#[async_test]
async fn suit_engine_socks5_direct_and_request() -> std::io::Result<()> {
    set_var("RUST_LOG", "debug");
    let _ = env_logger::try_init();

    for i in 0..2 {
        let r = suit_engine_socks5_direct_and_request_block_or_non_block(i & 2 == 0).await;
        if let Err(e) = r {
            return Err(e);
        }
    }
    Ok(())
}

async fn suit_engine_socks5_direct_and_request_block_or_non_block(
    even: bool,
) -> std::io::Result<()> {
    info!(
        "start suit_engine_socks5_direct_and_request_baidu test, {}",
        even
    );

    let c: Config = get_config();
    let cc = c.clone();

    let mut se = SuitEngine::new(load_in_adder_by_str, load_out_adder_by_str);
    se.load_config(rucimp::Config { proxy_config: c });

    let ti = net::TransmissionInfo::default();
    let arc_ti = Arc::new(ti);
    let arc_tic = arc_ti.clone();

    let listen_future = async {
        info!("try start listen");
        if even {
            let r = se.block_run().await;
            //let r = block_on(se.block_run());
            //let r = join!(se.block_run()) ;
            //测试表明，只能用 await的形式 或 join, 若用 block_on 的形式则运行结果异常。

            info!("r {:?}", r);
        } else {
            let r = se.run().await;

            info!("r {:?}", r);
        }
    };

    let cl = cc.listen.get(0).unwrap();
    let listen_host = cl.host.clone().unwrap();
    let listen_port = cl.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future = f_dial_future(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    pin_mut!(listen_future, dial_future);

    select! {

        () = dial_future => {
            info!("dial finished first, will return , {:?}",arc_tic);
        },
        () = listen_future => {
            panic!("listen finished first");
        },
    }

    //task::sleep(Duration::from_secs(2)).await;

    Ok(())
}

#[async_test]
async fn suit_engine_socks5_direct_and_request_block_3_listen() -> std::io::Result<()> {
    let even = true;

    set_var("RUST_LOG", "debug");
    let _ = env_logger::try_init();

    info!(
        "start suit_engine_socks5_direct_and_request_baidu test, {}",
        even
    );

    let c: Config = get_nl_config(3);
    let cc = c.clone();

    let mut se = SuitEngine::new(load_in_adder_by_str, load_out_adder_by_str);
    se.load_config(rucimp::Config { proxy_config: c });

    let ti = net::TransmissionInfo::default();
    let arc_ti = Arc::new(ti);
    let arc_tic = arc_ti.clone();

    let listen_future = async {
        info!("try start listen");
        if even {
            let r = se.block_run().await;
            //let r = block_on(se.block_run());
            //let r = join!(se.block_run()) ;
            //测试表明，只能用 await的形式 或 join, 若用 block_on 的形式则运行结果异常。

            info!("r {:?}", r);
        } else {
            let r = se.run().await;

            info!("r {:?}", r);
        }
    };

    let cl = cc.listen.get(0).unwrap();
    let listen_host = cl.host.clone().unwrap();
    let listen_port = cl.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future = f_dial_future(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    pin_mut!(listen_future, dial_future);

    select! {

        () = dial_future => {
            info!("dial finished first, will return , {:?}",arc_tic);
        },
        () = listen_future => {
            panic!("listen finished first");
        },
    }

    //task::sleep(Duration::from_secs(2)).await;

    Ok(())
}

// 同时发起两个请求的情况
#[async_test]
async fn socks5_direct_and_request_2_async() -> std::io::Result<()> {
    set_var("RUST_LOG", "debug");
    let _ = env_logger::try_init();

    info!("start socks5_direct_and_request_baidu_2_async");

    let mut c = get_config();

    let mut lsuit = SuitStruct::from(c.listen.pop().unwrap());
    lsuit.set_behavior(ruci::map::ProxyBehavior::DECODE);

    let a = get_socks5_inadder(&lsuit).await;
    lsuit.push_mapper(Box::new(a));

    let csuit = SuitStruct::from(c.dial.pop().unwrap());
    let ti = net::TransmissionInfo::default();
    let arc_ti = Arc::new(ti);
    let arc_tic = arc_ti.clone();

    let alsuit = Arc::new(lsuit);
    let alsuitc = alsuit.clone();

    let listen_future = async {
        info!("try start listen");

        let r = listen_ser(alsuit, Arc::new(csuit), Some(arc_ti)).await;

        info!("r {:?}", r);
    };

    let cc = alsuitc.get_config().unwrap().clone();
    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future = f_dial_future(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();
    let dial_future2 = f_dial_future(2, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    pin_mut!(listen_future, dial_future, dial_future2);

    let mut i = 2;

    while i > 0 {
        select! {
            () = listen_future => {
                panic!("listen finished first");
            },
            () = dial_future => {
                info!("dial1 finished, {:?}",arc_tic);
                i -= 1;
            },
            () = dial_future2 => {
                info!("dial2 finished, {:?}",arc_tic);
                i -= 1;

            },
        }
    }

    info!("test ok, will return in 2 secs... ");

    task::sleep(Duration::from_secs(2)).await;

    Ok(())
}
