/*

has 2 sub mods for udp.

*/

/// udp 模块中, 使用同一端口监听 来自 user 和 others 的 信息
pub mod udp;
pub mod udp2;

use super::*;

use crate::{
    buf_to_ob,
    map::{self, MapResult, MapperBox, ProxyBehavior, ToMapperBox, CID},
    net::{self, Addr, Conn},
    user::{self, AsyncUserAuthenticator, PlainText, UsersMap},
    Name,
};
use anyhow::{Context, Ok};
use bytes::{Buf, BytesMut};
use futures::{executor::block_on, select};
use log::{debug, log_enabled, warn};
use macro_mapper::NoMapperExt;
use map::Stream;
use std::{
    cmp::min,
    io::{self, Error},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UdpSocket,
    task,
};

#[derive(Default, Clone)]
pub struct Config {
    pub support_udp: bool,
    pub user_whitespace_pass: Option<String>,
    pub user_passes: Option<Vec<PlainText>>,
}

impl ToMapperBox for Config {
    fn to_mapper_box(&self) -> MapperBox {
        let a = block_on(Server::new(self.clone()));
        Box::new(a)
    }
}

/// Server  未实现bind 命令。
///  support_udp开关udp associate的支持
///
/// 支持 AuthNone和 AuthUserPass
#[derive(Debug, Clone, NoMapperExt)]
pub struct Server {
    pub um: Option<UsersMap<PlainText>>,
    pub support_udp: bool,
}

impl Server {
    pub async fn new(option: Config) -> Self {
        let mut um = UsersMap::new();

        if let Some(user_whitespace_pass) = option.user_whitespace_pass {
            let u = PlainText::from(user_whitespace_pass);
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
                let uup = user::PlainText::new(u.user, u.pass);
                um.add_user(uup).await;
            }
        }

        Server {
            support_udp: option.support_udp,
            um: if um.len().await > 0 { Some(um) } else { None },
        }
    }

    pub async fn handshake(
        &self,
        cid: CID,
        mut base: Conn,
        pre_read_data: Option<bytes::BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
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
            let e1 = anyhow!("{}, socks5: failed to read hello, too short: {}", cid, n);

            return Err(e1);
        }
        //buf.truncate(n); //不能 truncate

        let v = buf[0];
        if v != VERSION5 {
            if log_enabled!(log::Level::Debug) {
                let e2 = anyhow!(
                    "{}, socks5: unsupported version: {}, buf as str: {}",
                    cid,
                    v,
                    String::from_utf8_lossy(&buf[..min(n, 256)])
                );

                buf.truncate(n);
                return Ok(MapResult::ebc(e2, buf, base));
            } else {
                let e2 = anyhow!("{}, socks5: unsupported version: {}", cid, v);

                buf.truncate(n);
                return Ok(MapResult::ebc(e2, buf, base));
            }
        }

        let nm = buf[1];
        let nmp2 = 2 + nm as usize;

        if nm == 0 || n < nmp2 {
            buf[0] = VERSION5;
            buf[1] = AUTH_NO_ACCEPTABLE;
            base.write_all(&buf[..2]).await?;

            let e3 = anyhow!(
                "{}, socks5: nmethods==0||n < 2+nmethods: {}, n={}",
                cid,
                nm,
                n
            );

            buf.truncate(n);
            return Ok(MapResult::ebc(e3, buf, base));
        }
        let (mut authed, mut dealt_none, mut dealt_pass) = (false, false, false);

        let server_has_user = self.um.is_some();
        let mut opt_e: Option<io::Error> = None;

        let mut remainn = n - nmp2;

        let mut the_user: Option<PlainText> = None;

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
                    base.write_all(&buf[..2]).await?;
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
                            format!("{}, socks5: configured with no password at all but got auth method AuthPassword",cid)
                        ));
                        continue;
                    }
                    buf[0] = VERSION5;
                    buf[1] = AUTH_PASSWORD;
                    base.write_all(&buf[..2]).await?;

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
                            "{}, socks5: parse auth request failed",
                            cid
                        )));

                        continue;
                    }

                    if ul + 2 > n {
                        opt_e = Some(Error::other(format!("{}, socks5: parse auth request failed, ulen too long but data too short, {}", cid ,n)));

                        continue;
                    }

                    let ubytes = &auth_bs[2..2 + ul];
                    let pl = auth_bs[2 + ul] as usize;

                    if ul + 2 + pl > n {
                        opt_e = Some(Error::other(format!("{}, socks5: parse auth request failed, ulen too long but data too short, {}",cid, n)));
                        continue;
                    }

                    let auth_bs_len = 2 + ul + 1 + pl;
                    remainn -= auth_bs_len;

                    let pbytes = &auth_bs[2 + ul + 1..auth_bs_len];

                    let thisup = PlainText::new(
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
                    if let Some(um) = &self.um {
                        if um.auth_user_by_authstr(thisup.auth_strs()).await.is_some() {
                            authed = true;
                            opt_e = None;

                            base.write_all(&[USERPASS_SUBNEGOTIATION_VERSION, SUCCESS])
                                .await?;

                            the_user = Some(thisup);

                            break;
                        }
                    }

                    const FAILURE_1: u8 = 1;
                    let _ = base
                        .write(&[USERPASS_SUBNEGOTIATION_VERSION, FAILURE_1])
                        .await;

                    buf.truncate(n);
                    let e = anyhow!("{}, socks5: auth failed, {}", cid, thisup.auth_strs());
                    return Ok(MapResult::ebc(e, buf, base));
                }
                _ => {} //忽视其它的 auth method
            }
        }

        if !authed {
            buf[0] = VERSION5;
            buf[1] = AUTH_NO_ACCEPTABLE;
            let _ = base.write(&buf[..2]).await;

            let e4 = anyhow!("{}, socks5: not authed:  {:?}", cid, opt_e);

            buf.truncate(n);
            return Ok(MapResult::ebc(e4, buf, base));
        }

        if remainn > 0 {
            //客户端把下一条信息和第一条信息合在一起发了过来

            //buf 为 BytesMut 时，直接advance

            buf.advance(n - remainn);
            n = remainn;
        } else {
            n = base
                .read(&mut buf)
                .await
                .with_context(|| "socks5 server read client cmd msg failed")?;
        }
        if n < 7 {
            let e = anyhow!("{}, socks5: read cmd part failed, msgTooShort: {}", cid, n);

            buf.truncate(n);
            return Ok(MapResult::ebc(e, buf, base));
        }
        if buf[0] != VERSION5 {
            let e = anyhow!("{}, socks5: stage2, wrong verson, {}", cid, buf[0]);

            buf.truncate(n);
            return Ok(MapResult::ebc(e, buf, base));
        }

        let cmd = buf[1];
        if cmd == CMD_BIND {
            let e = anyhow!("{}, socks5: unsuppoted command CMD_BIND", cid);

            buf.truncate(n);
            return Ok(MapResult::ebc(e, buf, base));
        }

        if cmd != CMD_UDPASSOCIATE && cmd != CMD_CONNECT {
            let e = anyhow!("{}, socks5: unsuppoted command, {}", cid, cmd);

            buf.truncate(n);
            return Ok(MapResult::ebc(e, buf, base));
        }

        let (mut l, mut off) = (2, 4);
        let mut ip: Option<IpAddr> = None;
        let mut is_name = false;
        match buf[3] {
            ATYP_IP4 => {
                const IPV4L: usize = 4;
                l += IPV4L;
                let bs: [u8; IPV4L] = buf[off..off + IPV4L]
                    .try_into()
                    .expect("buf slice to array");
                ip = Some(std::net::IpAddr::V4(Ipv4Addr::from(bs)));
            }
            ATYP_IP6 => {
                const IPV6L: usize = 16;
                l += IPV6L;

                let bs: [u8; IPV6L] = buf[off..off + IPV6L]
                    .try_into()
                    .expect("buf slice to array");
                ip = Some(std::net::IpAddr::V6(Ipv6Addr::from(bs)));
            }
            ATYP_DOMAIN => {
                l += buf[4] as usize;
                off = 5;
                is_name = true;
            }
            _ => {
                let e = anyhow!("{}, socks5: unknown address type: {}", cid, buf[3]);

                buf.truncate(n);
                return Ok(MapResult::ebc(e, buf, base));
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
            let e = anyhow!("{}, socks5: stage2, short of [port] part {}", cid, n);

            buf.truncate(n);
            return Ok(MapResult::ebc(e, buf, base));
        }

        //network octet order, 即大端序, 低地址的数是更重要的字节 (即要左移8的字节).

        let port = (buf[end - 2] as u16) << 8 | buf[end - 1] as u16;

        buf.advance(end);
        buf.truncate(remain as usize);

        if log_enabled!(log::Level::Debug) && remain > 0 {
            debug!("{}, socks5 server got earlydata,{}", cid, remain);
        }

        //如果name中实际是 123.123.123.123 这种值(或ipv6的样式)，这种情况很常见，
        //要尝试将其转成ip
        if let Some(name) = &name {
            use std::str::FromStr;
            if let std::result::Result::Ok(tip) = IpAddr::from_str(name) {
                ip = Some(tip)
            }
        }
        let ad = Addr::from("tcp", name, ip, port).map_err(|e| io::Error::other(e.to_string()))?;

        fn ou_to_oad(ou: Option<PlainText>) -> Option<Box<dyn map::Data>> {
            ou.map(|up| {
                let b: Box<dyn map::Data> = Box::new(up);
                b
            })
        }
        let d = ou_to_oad(the_user);

        if cmd == CMD_CONNECT {
            let _ = base.write(&*COMMMON_TCP_HANDSHAKE_REPLY).await?;

            return Ok(MapResult {
                a: Some(ad),
                b: buf_to_ob(buf),
                c: Stream::c(base),
                d, //将 该登录的用户信息 作为 额外信息 传回
                ..Default::default()
            });
        }
        if cmd == CMD_UDPASSOCIATE && self.support_udp {
            let mut mr = udp2::udp_associate(cid, base, ad).await?;
            mr.d = d;
            return Ok(mr);
        }

        Ok(MapResult {
            b: buf_to_ob(buf),
            c: Stream::c(base),
            e: Some(anyhow!("socks5 server: not supported cmd, {}", cmd)),
            ..Default::default()
        })
    }
}
impl Name for Server {
    fn name(&self) -> &'static str {
        "socks5_server"
    }
}
#[async_trait::async_trait]
impl map::Mapper for Server {
    async fn maps(
        &self,
        cid: CID,
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        match params.c {
            map::Stream::Conn(c) => {
                let r = self.handshake(cid, c, params.b).await;

                MapResult::from_result(r)
            }
            _ => MapResult::err_str("socks5 only support tcplike stream"),
        }
    }
}
