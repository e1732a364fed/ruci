pub mod deprecated;

use super::*;

use crate::{
    map::{self, MapResult, ProxyBehavior},
    net::{self, Addr, Conn},
    user::{self, AsyncUserAuthenticator, UserPass, UsersMap},
    Name,
};
use bytes::{Buf, BytesMut};
use futures::select;
use log::{debug, log_enabled, warn};
use std::{
    collections::HashMap,
    io::{self, Error},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UdpSocket,
    task,
};

async fn server_udp_associate(
    cid: u32,
    mut base: Conn,
    client_future_addr: net::Addr,
) -> io::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?; //random port provided by OS.
    let udp_sock_addr = socket.local_addr()?;
    let port = udp_sock_addr.port();

    //4个0为 BND.ADDR(4字节的ipv4) ,表示还是用原tcp的ip地址
    let reply = [
        VERSION5,
        SUCCESS,
        RSV,
        ATYP_IP4,
        0,
        0,
        0,
        0,
        (port >> 8) as u8, // BND.PORT(2字节)
        port as u8,
    ];
    base.write(&reply).await?;

    task::spawn(loop_listen_udp_for_certain_client(
        cid,
        base,
        client_future_addr,
        socket,
    ));

    Ok(())
}

pub async fn loop_listen_udp_for_certain_client(
    cid: u32,
    mut base: Conn,
    client_future_addr: net::Addr,
    udpso_created_to_listen_for_thisuser: UdpSocket,
) -> io::Result<()> {
    const CAP: usize = 1500; //todo: change this

    let mut buf2 = BytesMut::with_capacity(CAP);

    let mut user_raddr: IpAddr = client_future_addr.get_ip().unwrap();
    let mut user_port: u16 = client_future_addr.get_port();

    use futures::channel::mpsc::channel;
    use futures::sink::SinkExt;
    use futures::stream::StreamExt;
    use futures::FutureExt;
    let (mut tx, mut rx) = channel::<(Option<SocketAddr>, BytesMut, SocketAddr)>(20);

    let udpso_created_to_listen_for_thisuser = Arc::new(udpso_created_to_listen_for_thisuser);

    let udp = udpso_created_to_listen_for_thisuser.clone();

    //loop write to user or remote
    task::spawn(async move {
        let mut buf_w = BytesMut::with_capacity(CAP);

        loop {
            let x = rx.next().await;
            match x {
                Some((msg_was_from, buf, send_to)) => {
                    match msg_was_from {
                        Some(from) => {
                            // from a remote, to the user. the send_to is the user's address

                            buf_w.clear();
                            encode_udp_diagram(
                                net::Addr {
                                    addr: net::NetAddr::Socket(from),
                                    network: net::Network::UDP,
                                },
                                &mut buf_w,
                            )
                            .unwrap();

                            buf_w.extend_from_slice(&buf);

                            let r = udp.send_to(&buf_w, send_to).await;

                            if r.is_err() {
                                break;
                            }
                        }
                        None => {
                            // from the user, to remote. the buf is already decoded
                            let r = udp.send_to(&buf, send_to).await;

                            if r.is_err() {
                                break;
                            }
                        }
                    }
                }
                None => break,
            }
        }
    });

    //loop read from user or remote
    loop {
        select! {

            /*
            A UDP association terminates when the TCP connection that the UDP
            ASSOCIATE request arrived on terminates.
            */

            result = base.read(&mut buf2).fuse()  =>{
                if result.is_err(){
                    debug!("cid: {}, socks5 server, will end loop listen udp because of the read err of the tcp conn, {}", cid, result.unwrap());

                    let _ = tx.close().await;

                    break;
                }
            },
            default =>{
                let mut buf = BytesMut::zeroed(CAP);

                let (n, raddr) = udpso_created_to_listen_for_thisuser.recv_from(&mut buf).await?;
                buf.truncate(n);


                if user_raddr.is_unspecified() || raddr.ip().eq(&user_raddr) {

                    //user write to remote


                    if user_raddr.is_unspecified() {
                        user_raddr = raddr.ip();
                        user_port = raddr.port();
                    }

                    let a = decode_udp_diagram(&mut buf)?;

                    let sor = a.get_socket_addr_or_resolve();
                    if let Err(e) = sor{
                        warn!("can't convert to socketaddr, {}",e);
                        continue;
                    }
                    let so = sor.unwrap();

                    let x= tx.send(( None,buf, so)).await;
                    if let Err(e) = x{
                        return Err(io::Error::other(format!("{}",e)));
                    }


                }else{

                    // new data from some remote

                    let x= tx.send(( Some(raddr),buf,SocketAddr::new(user_raddr,user_port))).await;
                    if let Err(e) = x{
                        return Err(io::Error::other(format!("{}",e)));
                    }
                }
            }
        }
    }

    Ok(())
}

#[derive(Default)]
pub struct Config {
    pub support_udp: bool,
    pub user_whitespace_pass: Option<String>,
    pub user_passes: Option<Vec<UserPass>>,
}

/// InServer  未实现bind 命令。
///  support_udp开关udp associate的支持
///
/// 支持 AuthNone和 AuthUserPass
#[derive(Debug)]
pub struct Server {
    pub um: Option<UsersMap<UserPass>>,
    pub support_udp: bool,
}

impl Server {
    pub async fn new(option: Config) -> Self {
        let mut um = UsersMap::new();

        if let Some(user_whitespace_pass) = option.user_whitespace_pass {
            let u = UserPass::from(user_whitespace_pass);
            if u.strict_valid() {
                um.add_user(u).await;
            }
        }

        /*
        // 上面可化简为如下形式，不过要用 join_all , 否则异步执行没完成就退出了,导致之后马上检查内容会得到空值。所以实际上没怎么化简
        futures::future:: join_all(c.uuid
             .map(UserPass::from)
             .filter(|u| u.strict_valid())
             .map(|u| async { um.add_user(u).await })).await ;

          */

        let mut cu = option.user_passes.clone();
        if let Some(a) = cu.as_mut().filter(|a| !a.is_empty()) {
            while let Some(u) = a.pop() {
                let uup = user::UserPass::new(u.user, u.pass);
                um.add_user(uup).await;
            }
        }

        Server {
            support_udp: option.support_udp,
            um: if um.len().await > 0 { Some(um) } else { None },
        }
    }

    async fn handshake(
        &self,
        cid: u32,
        mut base: Conn,
        _: Option<Addr>,
        pre_read_data: Option<bytes::BytesMut>,
    ) -> io::Result<map::MapResult> {
        /*
           todo:
           本段代码 是verysimple中socks5代码的移植 ，修复了它的一些漏洞
           并通过了单元测试。
           不过因为函数太长，依然不是很简洁的实现，可能需要 重构
           使用 bytes 的 BytesMut 可以很好地重构，但因为 async_std中没有 read_buf 方法
           所以还是不方便
           所以使用 read_buf 方法的重构要在 tokio 分支进行了。

           旧实现没有使用 bytes_to_addr 函数，代码显得很繁琐(不过代码效率没有区别)
        */

        let mut buf: BytesMut;
        //let mut buf = [0u8; 1024];

        let mut n: usize;

        if let Some(pd) = pre_read_data {
            n = pd.len();
            buf = pd;
        } else {
            buf = BytesMut::zeroed(1024);
            n = base.read(&mut buf).await?;
        }

        if n < 3 {
            let e1 = Error::other(format!(
                "cid: {}, socks5: failed to read hello, too short: {}",
                cid, n
            ));

            return Err(e1);
        }

        let v = buf[0];
        if v != VERSION5 {
            let e2 = Error::other(format!("cid: {}, socks5: unsupported version: {}", cid, v));

            return Err(e2);
        }

        let nm = buf[1];
        let nmp2 = 2 + nm as usize;

        if nm == 0 || n < nmp2 as usize {
            buf[0] = VERSION5;
            buf[1] = AUTH_NO_ACCEPTABLE;
            base.write(&buf[..2]).await?;

            let e3 = Error::other(format!(
                "cid: {}, socks5: nmethods==0||n < 2+nmethods: {}, n={}",
                cid, nm, n
            ));

            return Err(e3);
        }
        let (mut authed, mut dealt_none, mut dealt_pass) = (false, false, false);

        let server_has_user = self.um.is_some();
        let mut opt_e: Option<io::Error> = None;

        let mut remainn = n - nmp2;

        let mut the_user: Option<UserPass> = None;

        for i in 2..nmp2 {
            let m = buf[i];
            match m {
                AUTH_NONE => {
                    if dealt_none {
                        break;
                    }
                    dealt_none = true;

                    if server_has_user {
                        continue;
                    }
                    buf[0] = VERSION5;
                    buf[1] = AUTH_NONE;
                    base.write(&buf[..2]).await?;
                    authed = true;
                    break;
                }
                AUTH_PASSWORD => {
                    if dealt_pass {
                        break;
                    }
                    dealt_pass = true;

                    if !server_has_user {
                        opt_e = Some(Error::other(
                            format!("cid: {}, socks5: configured with no password at all but got auth method AuthPassword",cid)
                        ));
                        continue;
                    }
                    buf[0] = VERSION5;
                    buf[1] = AUTH_PASSWORD;
                    base.write(&buf[..2]).await?;

                    let auth_bs: &[u8];

                    if n == nmp2 {
                        n = base.read(&mut buf).await?;

                        auth_bs = &buf[..n];
                        remainn = n;
                    } else {
                        auth_bs = &buf[3..n];
                        //如果 客户端是 把下一个回复连着第一个请求发来的，则
                        // 一定是只指定了一个 auth方法，所以 第一个请求的长度一定为3 (5 1 2)
                        // todo: 不过，不排除攻击者的行为
                    }

                    /*
                    https://datatracker.ietf.org/doc/html/rfc1929

                      +----+------+----------+------+----------+
                      |VER | ULEN |  UNAME   | PLEN |  PASSWD  |
                      +----+------+----------+------+----------+
                      | 1  |  1   | 1 to 255 |  1   | 1 to 255 |
                      +----+------+----------+------+----------+

                      The VER field contains the current version of the subnegotiation, which is X'01'
                    */

                    let ul = auth_bs[1] as usize;

                    if auth_bs.len() < 5 || auth_bs[0] != USERPASS_SUBNEGOTIATION_VERSION || ul == 0
                    {
                        opt_e = Some(Error::other(format!(
                            "cid: {}, socks5: parse auth request failed",
                            cid
                        )));

                        continue;
                    }

                    if ul + 2 > n {
                        opt_e = Some(Error::other(format!("cid: {}, socks5: parse auth request failed, ulen too long but data too short, {}", cid ,n)));

                        continue;
                    }

                    let ubytes = &auth_bs[2..2 + ul];
                    let pl = auth_bs[2 + ul] as usize;

                    if ul + 2 + pl > n {
                        opt_e = Some(Error::other(format!("cid: {}, socks5: parse auth request failed, ulen too long but data too short, {}",cid, n)));
                        continue;
                    }

                    let auth_bs_len = 2 + ul + 1 + pl;
                    remainn -= auth_bs_len;

                    let pbytes = &auth_bs[2 + ul + 1..auth_bs_len];

                    let thisup = UserPass::new(
                        String::from_utf8_lossy(ubytes).to_string(),
                        String::from_utf8_lossy(pbytes).to_string(),
                    );

                    /*
                     The server verifies the supplied UNAME and PASSWD, and sends the
                    following response:

                                         +----+--------+
                                         |VER | STATUS |
                                         +----+--------+
                                         | 1  |   1    |
                                         +----+--------+

                    A STATUS field of X'00' indicates success. If the server returns a
                    `failure' (STATUS value other than X'00') status, it MUST close the connection.
                    */

                    if self
                        .um
                        .as_ref()
                        .unwrap()
                        .auth_user_by_authstr(thisup.auth_strs())
                        .await
                        .is_some()
                    {
                        authed = true;
                        opt_e = None;

                        base.write(&[USERPASS_SUBNEGOTIATION_VERSION, SUCCESS])
                            .await?;

                        the_user = Some(thisup);

                        break;
                    }
                    let _ = base.write(&[USERPASS_SUBNEGOTIATION_VERSION, 1]).await;
                    return Err(Error::new(
                        io::ErrorKind::InvalidData,
                        format!("cid: {}, socks5: auth failed, {}", cid, thisup.auth_strs()),
                    ));
                }
                _ => {} //忽视其它的 auth method
            }
        }

        if !authed {
            buf[0] = VERSION5;
            buf[1] = AUTH_NO_ACCEPTABLE;
            let _ = base.write(&buf[..2]).await;

            let e4 = Error::other(format!("cid: {}, socks5: not authed:  {:?}", cid, opt_e));

            return Err(e4);
        }

        if remainn > 0 {
            //客户端把下一条信息和第一条信息合在一起发了过来
            /*
            // buf 为 [u8;1024] 的情况, 要进行一次从后搬到前的拷贝

                let right = &mut buf[n - remainn..n];

                //不能有多个可变借用，下面代码不行
                //&buf[..].copy_from_slice(&right[..remainn]);

                // 获取 right 的指针和长度
                let right_ptr = right.as_mut_ptr();

                // 将 right 的内容复制到 buf 的起点处
                unsafe {
                    std::ptr::copy(right_ptr, buf.as_mut_ptr(), remainn);
                }


             */

            //buf 为 BytesMut 时，直接advance

            buf.advance(n - remainn);
            n = remainn;
        } else {
            n = base.read(&mut buf).await?;
        }

        if n < 7 {
            return Err(Error::other(format!(
                "cid: {}, socks5: read cmd part failed, msgTooShort: {}",
                cid, n
            )));
        }
        if buf[0] != VERSION5 {
            return Err(Error::other(format!(
                "cid: {}, socks5: stage2, wrong verson, {}",
                cid, buf[0]
            )));
        }

        let cmd = buf[1];
        if cmd == CMD_BIND {
            return Err(Error::other(format!(
                "cid: {}, socks5: unsuppoted command CMD_BIND",
                cid
            )));
        }

        if cmd != CMD_UDPASSOCIATE && cmd != CMD_CONNECT {
            return Err(Error::other(format!(
                "cid: {}, socks5: unsuppoted command, {}",
                cid, cmd
            )));
        }

        let (mut l, mut off) = (2, 4);
        let mut ip: Option<IpAddr> = None;
        let mut is_name = false;
        match buf[3] {
            ATYP_IP4 => {
                const IPV4L: usize = 4;
                l += IPV4L;
                let bs: [u8; IPV4L] = buf[off..off + IPV4L].try_into().unwrap();
                ip = Some(std::net::IpAddr::V4(Ipv4Addr::from(bs)));
            }
            ATYP_IP6 => {
                const IPV6L: usize = 16;
                l += IPV6L;

                let bs: [u8; IPV6L] = buf[off..off + IPV6L].try_into().unwrap();
                ip = Some(std::net::IpAddr::V6(Ipv6Addr::from(bs)));
            }
            ATYP_DOMAIN => {
                l += buf[4] as usize;
                off = 5;
                is_name = true;
            }
            _ => {
                return Err(Error::other(format!(
                    "cid: {}, socks5: unknown address type: {}",
                    cid, buf[3]
                )));
            }
        }
        let name: Option<String> = if is_name {
            Some(String::from_utf8_lossy(&buf[off..off + l - 2]).to_string())
        } else {
            None
        };
        let end = off + l;
        let remain = n as i32 - end as i32;

        if remain < 0 {
            return Err(Error::other(format!(
                "cid: {}, socks5: stage2, short of [port] part {}",
                cid, n
            )));
        }

        //network octet order, 即大端序, 低地址的数是更重要的字节 (即要左移8的字节).

        let port = (buf[end - 2] as u16) << 8 | buf[end - 1] as u16;

        buf.advance(end);
        buf.truncate(remain as usize);

        if log_enabled!(log::Level::Debug) && remain > 0 {
            debug!("cid: {}, socks5, got earlydata,{}", cid, remain);
        }

        //如果name中实际是 123.123.123.123 这种值(或ipv6的样式)，这种情况很常见，
        //要尝试将其转成ip
        if is_name {
            use std::str::FromStr;
            match IpAddr::from_str(name.as_ref().unwrap()) {
                Ok(tip) => ip = Some(tip),
                _ => {}
            }
        }
        let ad = Addr::from("tcp", name, ip, port).map_err(|e| io::Error::other(e.to_string()))?;

        if cmd == CMD_CONNECT {
            let _ = base.write(&*COMMMON_TCP_HANDSHAKE_REPLY).await?;

            return Ok(MapResult {
                a: Some(ad),
                b: if buf.len() > 0 { Some(buf) } else { None },
                c: Some(base),
                d: the_user.map_or(None, |up| Some(map::AnyData::B(Box::new(up)))), //将 该登录的用户信息 作为 额外信息 传回
                e: None,
            });
        }
        if cmd == CMD_UDPASSOCIATE && self.support_udp {
            server_udp_associate(cid, base, ad).await?;

            return Ok(MapResult {
                a: None,
                b: if buf.len() > 0 { Some(buf) } else { None },
                c: None,
                d: Some(map::AnyData::B(Box::new(map::NewConnectionOptData {
                    new_connection: map::NewConnection::UdpConnection,
                    data: None,
                }))), //标记我们 采用了新的udp连接
                e: None,
            });
        }

        return Ok(MapResult {
            a: None,
            b: if buf.len() > 0 { Some(buf) } else { None },
            c: Some(base),
            d: None,
            e: Some(Error::other(format!("socks5: not supported cmd, {}", cmd))),
        });
    }
}
impl Name for Server {
    fn name(&self) -> &'static str {
        "socks5"
    }
}
#[async_trait::async_trait]
impl map::Mapper for Server {
    async fn maps(
        &self,
        cid: u32,
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        match params.c {
            map::Stream::TCP(c) => {
                let x = self.handshake(cid, c, params.a, params.b).await;

                MapResult::from_result(x)
            }
            _ => MapResult::err_str("socks5 only support tcplike stream"),
        }
    }
}
