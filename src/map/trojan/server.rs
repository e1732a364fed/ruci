use super::*;
use crate::{
    map::{self, Data, MapResult, Mapper, MapperBox, MapperExtFields, ToMapperBox, CID},
    net::{self, helpers, Network},
    user::{AsyncUserAuthenticator, UsersMap},
    utils, Name,
};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use bytes::{Buf, BytesMut};
use futures::executor::block_on;
use macro_mapper::*;
use tokio::io::AsyncReadExt;

#[derive(Default, Clone)]
pub struct Config {
    pub pass: Option<String>,
    pub passes: Option<Vec<String>>,
}

impl ToMapperBox for Config {
    fn to_mapper_box(&self) -> MapperBox {
        let a = block_on(Server::new(self.clone()));
        Box::new(a)
    }
}

#[mapper_ext_fields]
#[derive(Debug, Clone, MapperExt)]
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

        Server {
            um,
            ext_fields: Some(MapperExtFields::default()),
        }
    }

    pub async fn handshake(
        &self,
        cid: CID,
        mut base: net::Conn,
        ob: Option<BytesMut>,
    ) -> anyhow::Result<MapResult> {
        //根据 https://www.ihcblog.com/a-better-tls-obfs-proxy/
        //trojan的 CRLF 是为了模拟http服务器的行为, 所以此时不要一次性Read，而是要Read到CRLF为止

        const CAP: usize = 1024;
        let mut previous_read_len: usize;
        let mut buf = match ob {
            Some(b) => {
                previous_read_len = b.len();
                b
            }
            None => {
                previous_read_len = 0;
                BytesMut::zeroed(CAP)
            }
        };

        if previous_read_len < 17 {
            loop {
                //tracing::debug!(cid = %cid, "trojan loop read");
                let n = base
                    .read(&mut buf[previous_read_len..])
                    .await
                    .with_context(|| "trojan server read failed")?;

                if n == 0 {
                    tracing::debug!(cid = %cid, "trojan server loop read header got n=0, will break");
                    break;
                }

                let mut index_crlf = -1;
                let new_len = previous_read_len + n;
                for i in previous_read_len..new_len {
                    if buf[i..].starts_with(&[CR, LF]) {
                        index_crlf = i as i16;
                        break;
                    }
                }
                previous_read_len = new_len;

                if new_len >= CAP || index_crlf > 0 {
                    break;
                }
            }
        }

        if previous_read_len < 17 {
            //根据下面回答，HTTP的最小长度恰好是16字节，但是是0.9版本。1.0是18字节，1.1还要更长。总之 可以直接不回落
            //https://stackoverflow.com/questions/25047905/http-request-minimum-size-in-bytes/25065089

            return Err(anyhow!(
                "trojan fallback, msg too short, {}",
                previous_read_len
            ));
        }

        if previous_read_len < PASS_LEN + 8 + 1 {
            return Ok(MapResult::ebc(
                anyhow!("trojan handshake len too short"),
                buf,
                base,
            ));
        }

        buf.truncate(previous_read_len);

        let pass_part = buf.split_to(PASS_LEN);

        let hash_str = String::from_utf8_lossy(&pass_part);
        let mut trojan_hash = String::from("trojan:");
        trojan_hash.push_str(&hash_str);

        let opt_user = self.um.auth_user_by_authstr(&trojan_hash).await;

        if opt_user.is_none() {
            return Ok(MapResult::ebc(
                anyhow!("trojan hash not match, given hash_str is {}", hash_str),
                buf,
                base,
            ));
        }
        let crlf = buf.get_u16();
        if crlf != CRLF {
            return Ok(MapResult::ebc(
                anyhow!("trojan crlf wrong, {} ", crlf),
                buf,
                base,
            ));
        }
        let cmd_b = buf.get_u8();
        let mut is_udp = false;

        match cmd_b {
            CMD_CONNECT => {}
            CMD_UDPASSOCIATE => {
                is_udp = true;
            }
            CMD_MUX => {
                return Ok(MapResult::buf_err_str(
                    buf,
                    "trojan cmd MUX not implemented",
                ));
            }
            _ => {
                return Ok(MapResult::buf_err_str(
                    buf,
                    &format!("trojan cmd byte wrong, {}", cmd_b),
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
                    "trojan no suffix crlf field, 1byte left",
                ));
            }
            return Ok(MapResult::err_str("trojan no suffix crlf field"));
        }
        let supposed_crlf = buf.get_u16();
        if supposed_crlf != CRLF {
            return Ok(MapResult::buf_err_str(
                buf,
                &format!("trojan expect CRLF but is, {}", supposed_crlf),
            ));
        }

        fn ou_to_od(opt_user: Option<User>) -> Option<Box<dyn Data>> {
            opt_user.map(|up| {
                let u: Box<dyn Data> = Box::new(up);
                u
            })
        }

        let d = ou_to_od(opt_user);

        if is_udp {
            let u = udp::from(base);
            let mut mr = MapResult::new_u(u)
                .a(Some(ta))
                .b(utils::buf_to_ob(buf))
                .build();

            mr.d = d;
            Ok(mr)
        } else {
            let mut mr = MapResult::new_c(base)
                .a(Some(ta))
                .b(utils::buf_to_ob(buf))
                .build();
            mr.d = d;

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
                let r = self.handshake(cid, c, params.b).await;
                MapResult::from_result(r)
            }
            _ => MapResult::err_str("trojan only support tcplike stream"),
        }
    }
}
