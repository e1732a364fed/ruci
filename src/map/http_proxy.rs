/*!
Implements a [`Map`] for http proxy by section 5.2 of https://www.ietf.org/rfc/rfc2817.txt.


h2 also has a CONNECT method, but it is not commonly used for proxy purpose.
https://datatracker.ietf.org/doc/html/rfc7540#section-8.3
 */

use std::cmp::min;
use std::fmt::Display;

use anyhow::{anyhow, bail};
use base64::prelude::*;
use bytes::BytesMut;
use futures::executor::block_on;
use macro_map::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;
use user_trait::{PlainText, UserAuthenticator, UsersMap};

use crate::map::{self, MapExt, MapResult};
use crate::net::http::Method;
use crate::net::CID;
// use crate::user::{self, AsyncUserAuthenticator};
use crate::net::{self, Conn};
use crate::utils::buf_to_ob;

use super::{Map, MapBox, MapExtFields, Stream};

pub const CONNECT_REPLY_STR: &str = "HTTP/1.1 200 Connection established\r\n\r\n";
pub const BASIC_AUTH_VALUE_PREFIX: &str = "Basic ";
pub const PROXY_AUTH_HEADER_STR: &str = "Proxy-Authorization ";

#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct Server {
    pub um: Option<UsersMap<PlainText>>,
    pub only_connect: bool,
}

impl Display for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "http_proxy_server")
    }
}

#[derive(Default, Clone)]
pub struct ServerConfig {
    pub only_support_connect: bool,
    pub user_whitespace_pass: Option<String>,
    pub user_passes: Option<Vec<PlainText>>,
}

impl From<ServerConfig> for MapBox {
    fn from(value: ServerConfig) -> Self {
        let a = block_on(Server::new(value.clone()));
        Box::new(a)
    }
}

impl Server {
    pub async fn new(option: ServerConfig) -> Self {
        let mut um = UsersMap::default();

        if let Some(user_whitespace_pass) = option.user_whitespace_pass {
            let u = PlainText::from(user_whitespace_pass.as_str());
            if u.password_non_empty() {
                um.add_user(u);
            }
        }

        let mut cu = option.user_passes.clone();
        if let Some(a) = cu.as_mut().filter(|a| !a.is_empty()) {
            while let Some(u) = a.pop() {
                let uup = user_trait::PlainText::new(u.user, u.pass);
                um.add_user(uup);
            }
        }

        Server {
            only_connect: option.only_support_connect,
            um: if um.is_empty() { None } else { Some(um) },
            ext_fields: Some(MapExtFields::default()),
        }
    }

    pub async fn handshake(
        &self,
        _cid: CID,
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
        if r.parse_result != Ok(()) {
            if tracing::enabled!(tracing::Level::DEBUG) {
                let e1 = anyhow::anyhow!(
                    "http proxy: get method/path failed: {:?}, buf as str:\n{}\n",
                    r.parse_result,
                    String::from_utf8_lossy(&buf[..min(n, 64)])
                );

                return Ok(MapResult::ebc(e1, buf, base));
            }
            let e1 = anyhow::anyhow!("http proxy: get method/path failed: {:?}", r.parse_result);

            return Ok(MapResult::ebc(e1, buf, base));
        }

        // debug!("http get request: {:?}", r);

        let mut authed_user: Option<PlainText> = None;

        //todo: add test for auth
        if self.um.is_some() {
            let mut ok = false;
            for rh in r.headers.iter() {
                if rh.head == PROXY_AUTH_HEADER_STR {
                    if !rh.value.starts_with(BASIC_AUTH_VALUE_PREFIX) {
                        let e1 = anyhow::anyhow!(
                            "http proxy: auth value not start with BASIC_AUTH_VALUE_PREFIX: , {}",
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
                                "http proxy: base64 decode err: {e}, {}",
                                &rh.value
                            );
                            return Ok(MapResult::ebc(e1, buf, base));
                        }
                    };
                    let bs = bs.as_slice();
                    let colon_index = match bs.iter().position(|u| *u == b':') {
                        Some(i) => i,
                        None => {
                            let e1 = anyhow::anyhow!("http proxy: no colon, {}", &rh.value);
                            return Ok(MapResult::ebc(e1, buf, base));
                        }
                    };

                    let u = user_trait::PlainText::new(
                        String::from_utf8_lossy(&bs[..colon_index]).to_string(),
                        String::from_utf8_lossy(&bs[colon_index + 1..n]).to_string(),
                    );

                    if let Some(um) = &self.um {
                        if let Some(u) = um.auth_user_by_authstr(u.auth_str()) {
                            ok = true;
                            authed_user = Some(u);
                        };
                    }

                    break;
                }
            } //for header

            if !ok {
                let e1 = anyhow::anyhow!("http proxy: auth failed ,{:?}", &r);
                return Ok(MapResult::ebc(e1, buf, base));
            }
        }

        let is_connect = r.method == Method::CONNECT;

        let mut addr_str: String;
        if is_connect {
            addr_str = r.path;
        } else {
            if self.only_connect {
                let e = anyhow::anyhow!("http proxy: non-connect method not supported by config",);

                return Ok(MapResult::ebc(e, buf, base));
            }

            let ur = Url::parse(&r.path);
            let url = match ur {
                Ok(u) => u,
                Err(e) => {
                    let e1 = anyhow::anyhow!("http proxy: invalid url: {e}, {}", &r.path);
                    return Ok(MapResult::ebc(e1, buf, base));
                }
            };

            addr_str = url.authority().to_string();

            if !addr_str.contains(':') {
                addr_str += ":80";
            }
        }

        let ta = net::Addr::from_addr_str("tcp", &addr_str);
        let ta = match ta {
            Ok(a) => a,
            Err(e) => {
                let e1 = anyhow::anyhow!(
                    "http proxy: invalid url, can't convert to Addr: {e}, {}",
                    &addr_str
                );
                return Ok(MapResult::ebc(e1, buf, base));
            }
        };

        if is_connect {
            base.write_all(CONNECT_REPLY_STR.as_bytes()).await?;
        }

        let data = authed_user.map(|up| {
            let b: Box<dyn map::Data> = Box::new(up);
            b
        });

        Ok(MapResult {
            a: Some(ta),
            b: if is_connect {
                None
            } else {
                if r.body_start_index > buf.len() {
                    bail!(
                        "http proxy server got r.body_start_index > buf.len(),{},{}",
                        r.body_start_index,
                        buf.len()
                    )
                }
                buf_to_ob(buf.split_to(r.body_start_index))
            }, // Connect 方式 是没有 early data 的
            c: Stream::c(base),
            d: data, //将 该登录的用户信息 作为 额外信息 传回
            ..Default::default()
        })
    }
}

#[async_trait::async_trait]
impl Map for Server {
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
            _ => MapResult::from_err_str("http proxy only support tcplike stream"),
        }
    }
}

/// 使用 CONNECT
#[map_ext_fields]
#[derive(Debug, Clone, MapExt, Default)]
pub struct Client {}

impl Client {
    pub fn boxed() -> MapBox {
        Box::<Client>::default()
    }
}

impl Display for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "http_proxy_client")
    }
}
impl Client {
    pub async fn handshake(
        &self,
        _cid: CID,
        mut base: net::Conn,
        ta: net::Addr,
        mut first_payload: Option<BytesMut>,
    ) -> anyhow::Result<MapResult> {
        //see rfc2817, 5.2 Requesting a Tunnel with CONNECT

        let b = format!(
            "CONNECT {} HTTP/1.1\r\nHost: {}\r\n\r\n",
            ta.get_addr_str(),
            ta.get_addr_str()
        );
        let b = b.as_bytes();
        base.write_all(b).await?;
        base.flush().await?;

        let mut buf = BytesMut::zeroed(1024);

        let n: usize = base.read(&mut buf).await?;

        Ok(
            if n == CONNECT_REPLY_STR.len() && buf[..n].eq(CONNECT_REPLY_STR.as_bytes()) {
                if self.is_tail_of_chain() {
                    if let Some(b) = &first_payload {
                        if !b.is_empty() {
                            let r = base.write_all(b).await;
                            match r {
                                Ok(_) => first_payload = None,
                                Err(e) => return Ok(MapResult::from_e(e)),
                            }

                            //debug!("trojan client writing ed {}", bl);
                        }
                    }
                }

                MapResult::new_c(base).b(first_payload).build()
            } else {
                MapResult::from_e(anyhow!("len != CONNECT_REPLY_STR.len"))
            },
        )
    }
}

#[async_trait::async_trait]
impl Map for Client {
    async fn maps(
        &self,
        cid: CID,
        _behavior: map::ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        match params.c {
            map::Stream::Conn(c) => {
                if let Some(a) = params.a {
                    let r = self.handshake(cid, c, a, params.b).await;
                    MapResult::from_result(r)
                } else {
                    MapResult::from_err_str("http proxy client requires a target_addr, got None")
                }
            }
            _ => MapResult::from_err_str("http proxy only support tcplike stream"),
        }
    }
}

#[cfg(test)]
mod test {

    use std::time::Duration;

    use tokio::net::{TcpListener, TcpStream};

    use super::*;

    #[tokio::test]
    async fn connect() -> anyhow::Result<()> {
        let ser = Server::default();
        let c = Client::default();

        let listen_port = net::gen_random_higher_port();
        let listen_host_str = "127.0.0.1";

        let jh = tokio::spawn(async move {
            let listener =
                TcpListener::bind(listen_host_str.to_string() + ":" + &listen_port.to_string())
                    .await
                    .unwrap();

            let (nc, _raddr) = listener.accept().await.unwrap();

            let r = ser.handshake(CID::default(), Box::new(nc), None).await;
            // println!("server: {r:?}",)
            match r {
                Ok(r) => r,
                Err(e) => panic!("{e}"),
            }
        });

        tokio::time::sleep(Duration::from_millis(500)).await;

        let cs = TcpStream::connect((listen_host_str, listen_port))
            .await
            .unwrap();

        let ta = net::Addr::from_addr_str("tcp", "www.baidu.com:80").unwrap();

        let r = c
            .handshake(CID::default(), Box::new(cs), ta.clone(), None)
            .await;

        //println!("client: {r:?}",);

        match r {
            Ok(_cr) => {
                let serr = jh.await.unwrap();
                // println!("sera: {:?}", serr.a);
                assert_eq!(serr.a.unwrap(), ta)
            }
            Err(e) => panic!("{e}"),
        }

        Ok(())
    }
}
