/*!
 * imp http proxy as map::Mapper
 */

use std::cmp::min;

use base64::prelude::*;
use bytes::BytesMut;
use futures::executor::block_on;
use log::log_enabled;
use macro_mapper::NoMapperExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

use crate::buf_to_ob;
use crate::map::{self, MapResult};
use crate::net::http::Method;
use crate::net::CID;
use crate::user::{self, AsyncUserAuthenticator, User};
use crate::{
    net::{self, http::FailReason, Conn},
    user::{PlainText, UsersMap},
    Name,
};

use super::{MapperBox, Stream, ToMapper, VecAnyData};

pub const CONNECT_REPLY_STR: &str = "HTTP/1.1 200 Connection established\r\n\r\n";
pub const BASIC_AUTH_VALUE_PREFIX: &str = "Basic ";
pub const PROXY_AUTH_HEADER_STR: &str = "Proxy-Authorization ";

#[derive(Debug, Clone, NoMapperExt)]
pub struct Server {
    pub um: Option<UsersMap<PlainText>>,
    pub only_connect: bool,
}

impl Name for Server {
    fn name(&self) -> &'static str {
        "http_proxy_server"
    }
}

#[derive(Default, Clone)]
pub struct Config {
    pub only_support_connect: bool,
    pub user_whitespace_pass: Option<String>,
    pub user_passes: Option<Vec<PlainText>>,
}

impl ToMapper for Config {
    fn to_mapper(&self) -> MapperBox {
        let a = block_on(Server::new(self.clone()));
        Box::new(a)
    }
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

        let mut cu = option.user_passes.clone();
        if let Some(a) = cu.as_mut().filter(|a| !a.is_empty()) {
            while let Some(u) = a.pop() {
                let uup = user::PlainText::new(u.user, u.pass);
                um.add_user(uup).await;
            }
        }

        Server {
            only_connect: option.only_support_connect,
            um: if um.len().await > 0 { Some(um) } else { None },
        }
    }

    pub async fn handshake(
        &self,
        cid: CID,
        mut base: Conn,
        pre_read_data: Option<bytes::BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        let mut buf: BytesMut;

        let n: usize;

        if let Some(pd) = pre_read_data {
            n = pd.len();
            buf = pd;
        } else {
            buf = BytesMut::zeroed(1024);
            n = base.read(&mut buf).await?;
            buf.truncate(n);
        }

        let r = net::http::parse_h1_request(&buf[..n], true);
        if r.fail_reason != FailReason::None {
            if log_enabled!(log::Level::Debug) {
                let e1 = anyhow::anyhow!(
                    "{cid}, http proxy: get method/path failed: {:?}, buf as str: {}",
                    r.fail_reason,
                    String::from_utf8_lossy(&buf[..min(n, 256)])
                );

                return Ok(MapResult::ebc(e1, buf, base));
            }
            let e1 = anyhow::anyhow!(
                "{cid}, http proxy: get method/path failed: {:?}",
                r.fail_reason
            );

            return Ok(MapResult::ebc(e1, buf, base));
        }
        let mut authed_user: Option<PlainText> = None;

        //todo: add test for auth
        if self.um.is_some() {
            let mut ok = false;
            for rh in r.headers.iter() {
                if rh.head == PROXY_AUTH_HEADER_STR {
                    if !rh.value.starts_with(BASIC_AUTH_VALUE_PREFIX) {
                        let e1 = anyhow::anyhow!(
                            "{cid}, http proxy: auth value not start with BASIC_AUTH_VALUE_PREFIX: , {}",
                            &rh.value
                        );
                        return Ok(MapResult::ebc(e1, buf, base));
                    }
                    let bsr = BASE64_STANDARD
                        .decode(&rh.value.as_bytes()[BASIC_AUTH_VALUE_PREFIX.len()..]);
                    let bs = match bsr {
                        Ok(b) => b,
                        Err(e) => {
                            let e1 = anyhow::anyhow!(
                                "{cid}, http proxy: base64 decode err: {e}, {}",
                                &rh.value
                            );
                            return Ok(MapResult::ebc(e1, buf, base));
                        }
                    };
                    let bs = bs.as_slice();
                    let colon_index = match bs.iter().position(|x| *x == b':') {
                        Some(i) => i,
                        None => {
                            let e1 = anyhow::anyhow!("{cid}, http proxy: no colon, {}", &rh.value);
                            return Ok(MapResult::ebc(e1, buf, base));
                        }
                    };

                    let u = user::PlainText::new(
                        String::from_utf8_lossy(&bs[..colon_index]).to_string(),
                        String::from_utf8_lossy(&bs[colon_index + 1..n]).to_string(),
                    );

                    if let Some(um) = &self.um {
                        if let Some(u) = um.auth_user_by_authstr(u.auth_strs()).await {
                            ok = true;
                            authed_user = Some(u);
                        };
                    }

                    break;
                }
            } //for header

            if !ok {
                let e1 = anyhow::anyhow!("{cid}, http proxy: auth failed ,{:?}", &r);
                return Ok(MapResult::ebc(e1, buf, base));
            }
        }

        let is_connect = r.method == Method::CONNECT;

        let mut addr_str: String;
        if is_connect {
            addr_str = r.path;
        } else {
            if self.only_connect {
                let e = anyhow::anyhow!(
                    "{cid}, http proxy: non-connect method not supported by config",
                );

                return Ok(MapResult::ebc(e, buf, base));
            }

            let ur = Url::parse(&r.path);
            let url = match ur {
                Ok(u) => u,
                Err(e) => {
                    let e1 = anyhow::anyhow!("{cid}, http proxy: invalid url: {e}, {}", &r.path);
                    return Ok(MapResult::ebc(e1, buf, base));
                }
            };

            addr_str = match url.host() {
                Some(h) => h.to_string(),
                None => {
                    let e1 = anyhow::anyhow!("{cid}, http proxy: no host in url: , {}", &r.path);
                    return Ok(MapResult::ebc(e1, buf, base));
                }
            };

            if !addr_str.contains(':') {
                addr_str += ":80";
            }
        }

        let ta = net::Addr::from_addr_str("tcp", &addr_str);
        let ta = match ta {
            Ok(a) => a,
            Err(e) => {
                let e1 = anyhow::anyhow!(
                    "{cid}, http proxy: invalid url, can't convert to Addr: {e}, {}",
                    &addr_str
                );
                return Ok(MapResult::ebc(e1, buf, base));
            }
        };

        if is_connect {
            base.write_all(CONNECT_REPLY_STR.as_bytes()).await?;
        }

        let data = authed_user.map(|up| {
            let b: Box<dyn User> = Box::new(up);
            map::AnyData::User(b)
        });
        let output_data = VecAnyData::from_opt_any(data);

        Ok(MapResult {
            a: Some(ta),
            b: buf_to_ob(buf),
            c: Stream::c(base),
            d: output_data, //将 该登录的用户信息 作为 额外信息 传回
            ..Default::default()
        })
    }
}

#[async_trait::async_trait]
impl map::Mapper for Server {
    async fn maps(
        &self,
        cid: CID,
        _behavior: map::ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        match params.c {
            map::Stream::Conn(c) => {
                let r = self.handshake(cid, c, params.b).await;

                MapResult::from_result(r)
            }
            _ => MapResult::err_str("http proxy only support tcplike stream"),
        }
    }
}
