use std::cmp::min;
use std::io::{self, Error};

use base64::prelude::*;
use bytes::BytesMut;
use futures::executor::block_on;
use log::log_enabled;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

use crate::map::{self, MapResult};
use crate::net::http::Method;
use crate::net::CID;
use crate::user::{self, AsyncUserAuthenticator};
use crate::{
    net::{self, http::FailReason, Conn},
    user::{UserPass, UsersMap},
    Name,
};

use super::{MapperBox, ToMapper};

pub const CONNECT_REPLY_STR: &str = "HTTP/1.1 200 Connection established\r\n\r\n";
pub const BASIC_AUTH_VALUE_PREFIX: &str = "Basic ";
pub const PROXY_AUTH_HEADER_STR: &str = "Proxy-Authorization ";

#[derive(Debug, Clone)]
pub struct Server {
    pub um: Option<UsersMap<UserPass>>,
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
    pub user_passes: Option<Vec<UserPass>>,
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
            let u = UserPass::from(user_whitespace_pass);
            if u.strict_valid() {
                um.add_user(u).await;
            }
        }

        let mut cu = option.user_passes.clone();
        if let Some(a) = cu.as_mut().filter(|a| !a.is_empty()) {
            while let Some(u) = a.pop() {
                let uup = user::UserPass::new(u.user, u.pass);
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
    ) -> io::Result<map::MapResult> {
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
                let e1 = Error::other(format!(
                    "{cid}, http proxy: get method/path failed: {:?}, buf as str: {}",
                    r.fail_reason,
                    String::from_utf8_lossy(&buf[..min(n, 256)])
                ));

                return Ok(MapResult::ebc(e1, buf, base));
            } else {
                let e1 = Error::other(format!(
                    "{cid}, http proxy: get method/path failed: {:?}",
                    r.fail_reason
                ));

                return Ok(MapResult::ebc(e1, buf, base));
            }
        }
        let mut authed_user: Option<UserPass> = None;
        if self.um.is_some() {
            let mut ok = false;
            for rh in r.headers.iter() {
                if rh.head == PROXY_AUTH_HEADER_STR {
                    if !rh.value.starts_with(BASIC_AUTH_VALUE_PREFIX) {
                        let e1 = Error::other(format!(
                            "{cid}, http proxy: auth value not start with BASIC_AUTH_VALUE_PREFIX: , {}",
                            &rh.value
                        ));
                        return Ok(MapResult::ebc(e1, buf, base));
                    }
                    let bsr = BASE64_STANDARD
                        .decode(&rh.value.as_bytes()[BASIC_AUTH_VALUE_PREFIX.len()..]);
                    let bs = match bsr {
                        Ok(b) => b,
                        Err(e) => {
                            let e1 = Error::other(format!(
                                "{cid}, http proxy: base64 decode err: {e}, {}",
                                &rh.value
                            ));
                            return Ok(MapResult::ebc(e1, buf, base));
                        }
                    };
                    let bs = bs.as_slice();
                    let colon_index = match bs.iter().position(|x| *x == b':') {
                        Some(i) => i,
                        None => {
                            let e1 =
                                Error::other(format!("{cid}, http proxy: no colon, {}", &rh.value));
                            return Ok(MapResult::ebc(e1, buf, base));
                        }
                    };

                    let u = user::UserPass::new(
                        String::from_utf8_lossy(&bs[..colon_index]).to_string(),
                        String::from_utf8_lossy(&bs[colon_index + 1..n]).to_string(),
                    );

                    match self
                        .um
                        .as_ref()
                        .unwrap()
                        .auth_user_by_authstr(u.auth_strs())
                        .await
                    {
                        Some(u) => {
                            ok = true;
                            authed_user = Some(u);
                        }
                        None => {}
                    };
                    break;
                }
            } //for header

            if !ok {
                let e1 = Error::other(format!("{cid}, http proxy: auth failed ,{:?}", &r));
                return Ok(MapResult::ebc(e1, buf, base));
            }
        }

        let is_connect = r.method == Method::CONNECT;

        let mut addr_str: String;
        if is_connect {
            addr_str = r.path;
        } else {
            if self.only_connect {
                let e = Error::other(format!(
                    "{cid}, http proxy: non-connect method not supported by config",
                ));

                return Ok(MapResult::ebc(e, buf, base));
            }

            let ur = Url::parse(&r.path);
            let url = match ur {
                Ok(u) => u,
                Err(e) => {
                    let e1 =
                        Error::other(format!("{cid}, http proxy: invalid url: {e}, {}", &r.path));
                    return Ok(MapResult::ebc(e1, buf, base));
                }
            };

            addr_str = match url.host() {
                Some(h) => h.to_string(),
                None => {
                    let e1 =
                        Error::other(format!("{cid}, http proxy: no host in url: , {}", &r.path));
                    return Ok(MapResult::ebc(e1, buf, base));
                }
            };

            if !addr_str.contains(":") {
                addr_str += ":80";
            }
        }

        let ta = net::Addr::from_addr_str("tcp", &addr_str);
        let ta = match ta {
            Ok(a) => a,
            Err(e) => {
                let e1 = Error::other(format!(
                    "{cid}, http proxy: invalid url, can't convert to Addr: {e}, {}",
                    &addr_str
                ));
                return Ok(MapResult::ebc(e1, buf, base));
            }
        };

        if is_connect {
            base.write(CONNECT_REPLY_STR.as_bytes()).await?;
        }

        return Ok(MapResult {
            a: Some(ta),
            b: if buf.len() > 0 { Some(buf) } else { None },
            c: map::Stream::TCP(base),
            d: authed_user.map_or(None, |up| Some(map::AnyData::B(Box::new(up)))), //将 该登录的用户信息 作为 额外信息 传回
            e: None,
            new_id: None,
        });
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
            map::Stream::TCP(c) => {
                let r = self.handshake(cid, c, params.b).await;

                MapResult::from_result(r)
            }
            _ => MapResult::err_str("http proxy only support tcplike stream"),
        }
    }
}
