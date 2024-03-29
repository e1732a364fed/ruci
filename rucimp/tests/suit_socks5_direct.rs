/*!
 * 集成测试 socks5 -> direct 的情况, 采用了随机端口, 以百度为目标网页
测试了 earlydata 的情况
测试了 同时监听三个端口 的情况
测试了 不监视流量 的情况  (no_transmission_info)
测试了 监视实际流量的情况 (counter)
测试了 直接调用 rucimp::suit::engine::relay::listen_ser 和 用 rucimp::suit::engine::SuitEngine 的情况
测试了 SuitEngine 的 block_run 和 run 两种情况
测试了 同时发起两个请求的情况 (异步)
测试了 发起长时间挂起的请求的情况 （非法）
 */

use std::{env::set_var, sync::Arc, time::Duration};

use crate::net::CID;
use bytes::{BufMut, BytesMut};
use futures::{pin_mut, select, Future, FutureExt};
use parking_lot::Mutex;
use ruci::map::socks5;
use ruci::map::socks5::*;
use ruci::net::GlobalTrafficRecorder;
use ruci::{map::Map, net, user::PlainText};
use rucimp::modes::suit::config::adapter::{
    load_in_maps_by_str_and_ld_config, load_out_maps_by_str_and_ld_config,
};
use rucimp::modes::suit::config::{Config, LDConfig};
use rucimp::modes::suit::engine::{listen_ser, SuitEngine};
use rucimp::modes::suit::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot::{self, Sender};
use tracing::{info, warn};

const WAITSECS: u64 = ruci::relay::READ_HANDSHAKE_TIMEOUT + 2;
const WAITID: i32 = 10101;

const TARGET_PORT: u16 = 80;
const TARGET_NAME: &str = "www.baidu.com";

fn init_log() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    let _ = tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init();
}

//参见 src/socks5/test.rs
async fn f_dial_future(
    rid: i32,
    listen_host_str: &str,
    listen_port: u16,
    the_target_name: &str,
    the_target_port: u16,
) {
    info!("start run f_dial_future, {}", rid);

    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut cs = TcpStream::connect((listen_host_str, listen_port))
        .await
        .unwrap();

    let mut readbuf = [0u8; 1024];

    if rid == WAITID {
        tokio::time::sleep(Duration::from_secs(WAITSECS)).await;
    }

    cs.write(&[VERSION5, 1, AUTH_PASSWORD]).await.unwrap();

    let n = cs.read(&mut readbuf[..]).await.unwrap();
    info!("client{} read, {:?}", rid, &readbuf[..n]);

    assert_eq!(&readbuf[..n], &[5, 2]);
    if rid == WAITID {
        tokio::time::sleep(Duration::from_secs(WAITSECS)).await;
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
        tokio::time::sleep(Duration::from_secs(WAITSECS)).await;
    }
    cs.write(&buf).await.unwrap();

    let n = cs.read(&mut readbuf[..]).await.unwrap();
    info!("client{} read, {:?}", rid, &readbuf[..n]);
    assert_eq!(&readbuf[..n], &*socks5::COMMON_TCP_HANDSHAKE_REPLY);

    info!("client{} writing hello...", rid,);
    //发送测试数据
    if rid == WAITID {
        tokio::time::sleep(Duration::from_secs(WAITSECS)).await;
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
) -> anyhow::Result<()> {
    info!("start run f_dial_future, {}", rid);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let cs = TcpStream::connect((listen_host_str, listen_port))
        .await
        .unwrap();

    let mut readbuf = [0u8; 1024];

    if rid == WAITID {
        tokio::time::sleep(Duration::from_secs(WAITSECS)).await;
    }

    let a = socks5::client::Client {
        up: Some(PlainText::from("u0 p0".to_string())),
        use_earlydata: false,
        ..Default::default()
    };
    let newconn = a
        .maps(
            CID::default(),
            ruci::map::ProxyBehavior::ENCODE,
            ruci::map::MapParams::ca(
                Box::new(cs),
                net::Addr::from_strs("tcp", the_target_name, "", the_target_port)?,
            ),
        )
        .await
        .c;

    info!("client{} writing hello...", rid,);

    let mut newconn = newconn.try_unwrap_tcp().expect("last_result as c");

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
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut cs = TcpStream::connect((listen_host_str, listen_port))
        .await
        .unwrap();

    let mut readbuf = [0u8; 1024];

    // if rid == WAITID {
    //     tokio::time::sleep(Duration::from_secs(WAITSECS)).await;
    // }

    cs.write(&[VERSION5, 1, AUTH_PASSWORD]).await.unwrap();

    let n = cs.read(&mut readbuf[..]).await.unwrap();
    info!("client{} read, {:?}", rid, &readbuf[..n]);

    assert_eq!(&readbuf[..n], &[5, 2]);
    // if rid == WAITID {
    //     tokio::time::sleep(Duration::from_secs(WAITSECS)).await;
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
    //     tokio::time::sleep(Duration::from_secs(WAITSECS)).await;
    // }
    cs.write(&buf).await.unwrap();

    let n = cs.read(&mut readbuf[..]).await.unwrap();
    info!("client{} read, {:?}", rid, &readbuf[..n]);
    assert_eq!(&readbuf[..n], &*socks5::COMMON_TCP_HANDSHAKE_REPLY);

    //info!("client{} writing hello...", rid,);
    //发送测试数据
    // if rid == WAITID {
    //     tokio::time::sleep(Duration::from_secs(WAITSECS)).await;
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

    toml_str.replace("12345", &ps)
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
    toml::from_str(&ws).unwrap()
}

async fn get_socks5_map(lsuit: &SuitStruct) -> socks5::server::Server {
    socks5::server::Server::new(
        rucimp::modes::suit::config::adapter::get_socks5_server_option_from_ld_config(
            lsuit.get_config().unwrap().clone(),
        ),
    )
    .await
}

async fn lisen_ser() -> anyhow::Result<(
    impl Future<Output = ()>,
    Sender<()>,
    Arc<GlobalTrafficRecorder>,
    LDConfig,
)> {
    let mut c = get_config();

    let mut lsuit = SuitStruct::from(c.listen.pop().unwrap());
    lsuit.set_behavior(ruci::map::ProxyBehavior::DECODE);

    let a = get_socks5_map(&lsuit).await;
    lsuit.push_map(Arc::new(Box::new(a)));

    let csuit = SuitStruct::from(c.dial.pop().unwrap());

    let gtr = net::GlobalTrafficRecorder::default();
    let arc_ti = Arc::new(gtr);
    let arc_tic = arc_ti.clone();

    let (tx, rx) = oneshot::channel();

    let alsuitc: Arc<Box<dyn Suit>> = Arc::new(Box::new(lsuit));
    let alsuitcc = alsuitc.clone();

    let listen_future = async {
        info!("try start listen");

        let r = listen_ser(alsuitc, Arc::new(Box::new(csuit)), Some(arc_ti), rx).await;

        info!("r {:?}", r);
    };
    Ok((
        listen_future,
        tx,
        arc_tic,
        alsuitcc.get_config().unwrap().clone(),
    ))
}

/// 基本测试. 百度在遇到非http请求后会主动断开连接, 其对于长挂起请求最多60秒后断开连接.
/// 其对请求中不含\n 时会视为挂起
#[tokio::test]
async fn socks5_direct_and_request() -> anyhow::Result<()> {
    set_var("RUST_LOG", "debug");
    init_log();

    info!("start socks5_direct_and_request_baidu test");

    let (listen_future, tx, arc_tic, cc) = lisen_ser().await?;

    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future = f_dial_future(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    let sleep_f = tokio::time::sleep(Duration::from_secs(100)).fuse();
    pin_mut!(listen_future, dial_future, sleep_f);

    /*

    baidu应返回 : HTTP/1.1 400 Bad Request\r\n\r\n

    copy 中的 half2 会先返回, 即从 到百度的连接 到 到socks5的连接 的拷贝 会断开（到百度的连接 是 被百度自动断开的）

     */

    select! {
        () = listen_future => {
            panic!("listen finished first");
        },
        () = dial_future => {
            info!("dial finished first , {:?}",arc_tic);

            //wait the second part of copy to end so that we can see the correct db debug info from net::cp
            tokio::time::sleep(Duration::from_millis(400)).await;
            info!("dial finished first ,print again, {:?}",arc_tic);
        },
        _ = sleep_f =>{

            let _ = tx.send(());
        }
    }

    Ok(())
}

#[tokio::test]
async fn socks5_direct_and_outadder() -> anyhow::Result<()> {
    set_var("RUST_LOG", "debug");
    init_log();

    info!("start socks5_direct_and_request_baidu test");

    let (listen_future, tx, arc_tic, cc) = lisen_ser().await?;

    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future =
        f_dial_future_out_adder(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    let sleep_f = tokio::time::sleep(Duration::from_secs(100)).fuse();
    pin_mut!(listen_future, dial_future, sleep_f);

    select! {
        () = listen_future => {
            panic!("listen finished first");
        },
        r = dial_future => {
            info!("dial finished first , {:?}",r);
            tokio::time::sleep(Duration::from_millis(400)).await;
            info!("dial finished first ,print again, {:?}",arc_tic);

        },
        _ = sleep_f =>{

            let _ = tx.send(());
        }
    }
    Ok(())
}

/// 不监视原始流量, 性能会高一些
#[tokio::test]
async fn socks5_direct_and_request_no_transmission_info() -> anyhow::Result<()> {
    set_var("RUST_LOG", "debug");
    init_log();

    info!("start socks5_direct_and_request_baidu test");

    let (listen_future, tx, _arc_tic, cc) = lisen_ser().await?;

    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future = f_dial_future(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    let sleep_f = tokio::time::sleep(Duration::from_secs(100)).fuse();
    pin_mut!(listen_future, dial_future, sleep_f);

    select! {
        () = listen_future => {
            panic!("listen finished first");
        },
        () = dial_future => {
            info!("dial finished first ", );

        },
        _ = sleep_f =>{

            let _ = tx.send(());
        }

    }

    Ok(())
}

/// 不监视原始流量但是监视实际流量, 用 加一层 InCounter 实现.
///
/// 注: 这里就体现了链式代理的特点. 可以里一层counter 外一层counter 如 counter - socks5 - counter 来分别记录原始流量
/// 和实际流量
#[tokio::test]
async fn socks5_direct_and_request_counter() -> anyhow::Result<()> {
    set_var("RUST_LOG", "debug");
    init_log();

    info!("start socks5_direct_and_request_baidu test");

    let mut c = get_config();

    let mut lsuit = SuitStruct::from(c.listen.pop().unwrap());
    lsuit.set_behavior(ruci::map::ProxyBehavior::DECODE);

    let a = ruci::map::counter::Counter::default();
    lsuit.push_map(Arc::new(Box::new(a)));

    let a = get_socks5_map(&lsuit).await;
    lsuit.push_map(Arc::new(Box::new(a)));

    let wn = lsuit.whole_name.clone();

    let csuit = SuitStruct::from(c.dial.pop().unwrap());
    let cc = lsuit.get_config().unwrap().clone();

    let (tx, rx) = oneshot::channel();

    let listen_future = async {
        info!("try start listen, {}", wn);

        let r = listen_ser(
            Arc::new(Box::new(lsuit)),
            Arc::new(Box::new(csuit)),
            None,
            rx,
        )
        .await;

        info!("r {:?}", r);
    };

    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future = f_dial_future(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    let sleep_f = tokio::time::sleep(Duration::from_secs(100)).fuse();
    pin_mut!(listen_future, dial_future, sleep_f);

    select! {
        () = listen_future => {
            panic!("listen finished first");
        },
        () = dial_future => {
            info!("dial finished first ", );

        },
        _ = sleep_f =>{

            let _ = tx.send(());
        }

    }

    Ok(())
}

#[tokio::test]
async fn socks5_direct_and_request_earlydata() -> anyhow::Result<()> {
    set_var("RUST_LOG", "debug");
    init_log();

    info!("start socks5_direct_and_request_baidu test");

    let (listen_future, tx, arc_tic, cc) = lisen_ser().await?;

    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future =
        f_dial_future_earlydata(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    let sleep_f = tokio::time::sleep(Duration::from_secs(100)).fuse();
    pin_mut!(listen_future, dial_future, sleep_f);

    select! {
        () = listen_future => {
            panic!("listen finished first");
        },
        () = dial_future => {
            info!("dial finished first , {:?}",arc_tic);
            tokio::time::sleep(Duration::from_millis(400)).await;
            info!("dial finished first ,print again, {:?}",arc_tic);

        },
        _ = sleep_f =>{

            let _ = tx.send(());
        }

    }

    Ok(())
}

//因为太耗时, 所以test被注释掉
/// 每次write前等待 ruci::proxy::READ_HANDSHAKE_TIMEOUT + 2 秒
#[tokio::test]
#[should_panic]
#[allow(dead_code)]
async fn socks5_direct_longwait_write_and_request() {
    set_var("RUST_LOG", "debug");
    init_log();
    set_var("RUST_BACKTRACE", "0");

    info!("start socks5_direct_and_request_baidu test");

    let (listen_future, tx, arc_tic, cc) = lisen_ser().await.expect("listen ok");
    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future =
        f_dial_future(WAITID, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    let sleep_f = tokio::time::sleep(Duration::from_secs(100)).fuse();
    pin_mut!(listen_future, dial_future, sleep_f);

    select! {
        () = listen_future => {
            panic!("listen finished first");
        },
        () = dial_future => {
            info!("dial finished first, will return in 2 secs... , {:?}",arc_tic);
            tokio::time::sleep(Duration::from_millis(400)).await;
            info!("dial finished first ,print again, {:?}",arc_tic);

        },
        _ = sleep_f =>{

            let _ = tx.send(());
        }

    }

    tokio::time::sleep(Duration::from_secs(2)).await;
}

/// 对 block_run 和 non_block run 各测一次
#[tokio::test]
async fn suit_engine_socks5_direct_and_request() -> anyhow::Result<()> {
    set_var("RUST_LOG", "debug");
    init_log();

    for i in 0..2 {
        let r = suit_engine_socks5_direct_and_request_block_or_non_block(i % 2 == 0).await;
        r?
    }
    Ok(())
}

async fn suit_engine_socks5_direct_and_request_block_or_non_block(
    even: bool,
) -> anyhow::Result<()> {
    info!(
        "start suit_engine_socks5_direct_and_request_block_or_non_block test, {}",
        even
    );

    let c: Config = get_config();
    let cc = c.clone();

    let mut se = SuitEngine::default();
    se.load_config(
        c,
        load_in_maps_by_str_and_ld_config,
        load_out_maps_by_str_and_ld_config,
    );

    let listen_future = async {
        if even {
            info!("try start listen block");

            let r = se.block_run().await;
            //let r = block_on(se.block_run());
            //let r = join!(se.block_run()) ;
            //测试表明, 只能用 await的形式 或 join, 若用 block_on 的形式则运行结果异常.

            info!("r {:?}", r);
        } else {
            info!("try start listen unblock");

            let r = se.run().await;

            info!("r {:?}", r);
        }
    };

    let cl = cc.listen.first().unwrap();
    let listen_host = cl.host.clone().unwrap();
    let listen_port = cl.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future = f_dial_future(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    pin_mut!(listen_future, dial_future);

    loop {
        select! {

            () = dial_future => {
                info!("dial finished first, will return , {:?}", se.gtr);
                tokio::time::sleep(Duration::from_millis(400)).await;
                info!("dial finished first ,print again, {:?}",se.gtr);

                break;
            },
            () = listen_future => {
                info!("listen finished first");
            },
        }
    }

    //tokio::time::sleep(Duration::from_secs(2)).await;

    Ok(())
}

#[tokio::test]
async fn suit_engine_socks5_direct_and_request_block_3_listen() -> anyhow::Result<()> {
    let even = true;

    set_var("RUST_LOG", "debug");
    init_log();

    info!(
        "start suit_engine_socks5_direct_and_request_baidu test, {}",
        even
    );

    let c: Config = get_nl_config(3);
    let cc = c.clone();

    let mut se = SuitEngine::default();
    se.load_config(
        c,
        load_in_maps_by_str_and_ld_config,
        load_out_maps_by_str_and_ld_config,
    );

    let listen_future = async {
        if even {
            info!("try start listen block run");

            let r = se.block_run().await;
            //let r = block_on(se.block_run());
            //let r = join!(se.block_run()) ;
            //测试表明, 只能用 await的形式 或 join, 若用 block_on 的形式则运行结果异常.

            info!("r {:?}", r);
        } else {
            info!("try start  listen unblock run");

            let r = se.run().await;

            info!("r {:?}", r);
        }
    };

    let cl = cc.listen.first().unwrap();
    let listen_host = cl.host.clone().unwrap();
    let listen_port = cl.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future = f_dial_future(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    pin_mut!(listen_future, dial_future);

    select! {

        () = dial_future => {
            info!("dial finished first, will return , {:?}",se.gtr);
            tokio::time::sleep(Duration::from_millis(400)).await;
            info!("dial finished first ,print again, {:?}",se.gtr);

        },
        () = listen_future => {
            panic!("listen finished first");
        },
    }

    //tokio::time::sleep(Duration::from_secs(2)).await;

    Ok(())
}

#[tokio::test]
async fn suit_engine2_socks5_direct_and_request_block_3_listen() -> anyhow::Result<()> {
    let even = true;

    set_var("RUST_LOG", "debug");
    init_log();

    info!(
        "start suit_engine_socks5_direct_and_request_baidu test, {}",
        even
    );

    let c: Config = get_nl_config(3);
    let cc = c.clone();

    let se = rucimp::modes::suit::engine::SuitEngine::default();
    let se = Arc::new(Mutex::new(Box::new(se)));
    {
        let mut lo = se.lock();
        lo.load_config(
            c,
            load_in_maps_by_str_and_ld_config,
            load_out_maps_by_str_and_ld_config,
        );
    }
    let sec = se.clone();

    let listen_future = async move {
        // 使用了 mutex 就不能用block run
        // if even {
        //     info!("try start listen block run");
        //     let lo = se.lock();

        //     tokio::select! {
        //         r = lo.block_run() =>{
        //             info!("listen finish {:?}", r);

        //         }
        //         _ = rx =>{
        //             info!("rx got");

        //             drop(lo);
        //         }
        //     }
        //     // let r = lo.block_run().await;
        //     //let r = block_on(se.block_run());
        //     //let r = join!(se.block_run()) ;
        //     //测试表明, 只能用 await的形式 或 join, 若用 block_on 的形式则运行结果异常.
        // } else {
        info!("try start  listen unblock run");
        let lo = se.lock();

        let r = lo.run().await;

        info!("r {:?}", r);
        //}
    };

    let cl = cc.listen.first().unwrap();
    let listen_host = cl.host.clone().unwrap();
    let listen_port = cl.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future = f_dial_future(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();

    pin_mut!(listen_future, dial_future);

    let mut dialfirstok = false;

    let x: &mut bool = &mut dialfirstok;
    loop {
        select! {

            () = dial_future => {
                info!("dial finished first,");
                *x = true;
                break;



            },
            () = listen_future => {
                info!("listen finished first");
            },
        }
    }

    if dialfirstok {
        info!("dial finished first, will return , {:?}", sec.lock().gtr);
        tokio::time::sleep(Duration::from_millis(400)).await;
        info!("dial finished first ,print again, {:?}", sec.lock().gtr);
    }

    //tokio::time::sleep(Duration::from_secs(2)).await;

    Ok(())
}

// 同时发起两个请求的情况
#[tokio::test]
async fn socks5_direct_and_request_2_async() -> anyhow::Result<()> {
    set_var("RUST_LOG", "debug");
    init_log();

    info!("start socks5_direct_and_request_baidu_2_async");

    let (listen_future, tx, arc_tic, cc) = lisen_ser().await?;

    let listen_host = cc.host.unwrap();
    let listen_port = cc.port.unwrap();

    let listen_future = listen_future.fuse();
    let dial_future = f_dial_future(1, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();
    let dial_future2 = f_dial_future(2, &listen_host, listen_port, TARGET_NAME, TARGET_PORT).fuse();
    let sleep_f = tokio::time::sleep(Duration::from_secs(100)).fuse();

    pin_mut!(listen_future, dial_future, dial_future2, sleep_f);

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
            _ = sleep_f =>{

                let _ = tx.send(());
                warn!("sleep timeout");
                break;
            }

        }
    }

    info!("test ok ");

    Ok(())
}
