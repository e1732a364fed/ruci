use super::*;
use crate::{
    map::{self, AnyData, MapResult, Mapper, MapperBox, ToMapper, CID},
    net::{self, helpers, Network},
    user::{self, AsyncUserAuthenticator, UsersMap},
    Name,
};
use anyhow::anyhow;
use async_trait::async_trait;
use bytes::{Buf, BytesMut};
use futures::executor::block_on;
use macro_mapper::NoMapperExt;
use tokio::io::AsyncReadExt;

#[derive(Default, Clone)]
pub struct Config {
    pub pass: Option<String>,
    pub passes: Option<Vec<String>>,
}

impl ToMapper for Config {
    fn to_mapper(&self) -> MapperBox {
        let a = block_on(Server::new(self.clone()));
        Box::new(a)
    }
}

#[derive(Debug, Clone, NoMapperExt)]
pub struct Server {
    pub um: UsersMap<User>,
}
impl Server {
    pub async fn new(option: Config) -> Self {
        let mut um = UsersMap::new();

        if let Some(u) = option.pass {
            let u = User::new(&u);
            um.add_user(u).await;
        }

        let mut cu = option.passes.clone();
        if let Some(a) = cu.as_mut().filter(|a| !a.is_empty()) {
            while let Some(u) = a.pop() {
                let uup = User::new(&u);
                um.add_user(uup).await;
            }
        }
        if um.len().await == 0 {
            panic!("can't init a trojan server without any password");
        }

        Server { um }
    }

    pub async fn handshake(&self, _cid: CID, mut base: net::Conn) -> anyhow::Result<MapResult> {
        //根据 https://www.ihcblog.com/a-better-tls-obfs-proxy/
        //trojan的 CRLF 是为了模拟http服务器的行为, 所以此时不要一次性Read，而是要Read到CRLF为止

        const CAP: usize = 1024;
        let mut buf = BytesMut::zeroed(CAP);
        let mut previous_read_len: usize = 0;
        loop {
            let n = base.read(&mut buf[previous_read_len..]).await?;

            let mut index_crlf = -1;
            let newlen = previous_read_len + n;
            for i in previous_read_len..newlen {
                if buf[i..].starts_with(&[CR, LF]) {
                    index_crlf = i as i16;
                    break;
                }
            }
            previous_read_len = newlen;

            if newlen >= CAP || index_crlf > 0 {
                break;
            }
        }

        if previous_read_len < 17 {
            //根据下面回答，HTTP的最小长度恰好是16字节，但是是0.9版本。1.0是18字节，1.1还要更长。总之 可以直接不回落
            //https://stackoverflow.com/questions/25047905/http-request-minimum-size-in-bytes/25065089

            return Err(anyhow!("fallback, msg too short, {}", previous_read_len));
        }

        if previous_read_len < PASS_LEN + 8 + 1 {
            return Ok(MapResult::buf_err_str(buf, "handshake len too short"));
        }

        buf.truncate(previous_read_len);

        let pass_part = buf.split_to(PASS_LEN);

        let hash_str = String::from_utf8_lossy(&pass_part);
        let mut trojan_hash = String::from("trojan:");
        trojan_hash.push_str(&hash_str);

        let opt_user = self.um.auth_user_by_authstr(&trojan_hash).await;

        if opt_user.is_none() {
            return Ok(MapResult::buf_err_str(buf, "hash not match"));
        }
        let crlf = buf.get_u16();
        if crlf != CRLF {
            return Ok(MapResult::buf_err_str(
                buf,
                &format!("crlf wrong, {} ", crlf),
            ));
        }
        let cmdb = buf.get_u8();
        let mut is_udp = false;

        match cmdb {
            CMD_CONNECT => {}
            CMD_UDPASSOCIATE => {
                is_udp = true;
            }
            CMD_MUX => {
                return Ok(MapResult::buf_err_str(buf, "cmd MUX not implemented"));
            }
            _ => {
                return Ok(MapResult::buf_err_str(
                    buf,
                    &format!("cmd byte wrong, {}", cmdb),
                ));
            }
        }

        let mut ta = match helpers::socks5_bytes_to_addr(&mut buf) {
            Ok(ta) => ta,
            Err(e) => return Ok(MapResult::buf_err(buf, e)),
        };

        if is_udp {
            ta.network = Network::UDP;
        }
        if buf.len() < 2 {
            if buf.len() == 1 {
                return Ok(MapResult::buf_err_str(
                    buf,
                    "no suffix crlf field, 1byte left",
                ));
            }
            return Ok(MapResult::err_str("no suffix crlf field"));
        }
        let supposed_crlf = buf.get_u16();
        if supposed_crlf != CRLF {
            return Ok(MapResult::buf_err_str(
                buf,
                &format!("expect CRLF but is, {}", supposed_crlf),
            ));
        }

        fn ou_to_od(opt_user: Option<User>) -> Option<AnyData> {
            opt_user.map(|up| {
                let b: Box<dyn user::User> = Box::new(up);
                map::AnyData::User(b)
            })
        }

        if is_udp {
            let u = udp::from(base);
            let mut mr = MapResult::newu(u).a(Some(ta)).b(Some(buf)).build();
            mr.d = ou_to_od(opt_user);
            Ok(mr)
        } else {
            let mut mr = MapResult::newc(base).a(Some(ta)).b(Some(buf)).build();
            mr.d = ou_to_od(opt_user);

            Ok(mr)
        }
    }
}
impl Name for Server {
    fn name(&self) -> &'static str {
        "trojan_server"
    }
}

#[async_trait]
impl Mapper for Server {
    async fn maps(
        &self,
        cid: CID,
        _behavior: map::ProxyBehavior,
        params: map::MapParams,
    ) -> MapResult {
        match params.c {
            map::Stream::Conn(c) => {
                let r = self.handshake(cid, c).await;
                MapResult::from_result(r)
            }
            _ => MapResult::err_str("trojan only support tcplike stream"),
        }
    }
}
