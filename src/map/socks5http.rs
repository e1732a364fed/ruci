/*!
Defines a [`Map`] that can accept both socks5 and http proxy request.

It will try socks5 first . If not socks5, fallbacks to http proxy
 */

use std::fmt::Display;

use macro_map::*;
use map::Stream;
use tracing::debug;

use super::{http_proxy, socks5, Map, MapBox, MapExtFields};
use crate::map::{self, MapResult};
use crate::net::Conn;
use crate::net::CID;
use user_trait::{PlainText, UsersMap};

#[derive(Default, Clone)]
pub struct Config {
    pub user_whitespace_pass: Option<String>,
    pub user_passes: Option<Vec<PlainText>>,
}

impl From<Config> for MapBox {
    fn from(value: Config) -> Self {
        let a = Server::new(value.clone());
        Box::new(a)
    }
}

#[map_ext_fields]
#[derive(Debug, Clone, MapExt)]
pub struct Server {
    pub http_s: http_proxy::Server,
    pub socks5_s: socks5::server::Server,
}

impl Display for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "socks5http_server")
    }
}

impl Server {
    pub fn boxed(option: Config) -> MapBox {
        Box::new(Self::new(option))
    }

    pub fn new(option: Config) -> Self {
        let mut um = UsersMap::default();

        if let Some(user_whitespace_pass) = option.user_whitespace_pass {
            let u = PlainText::from(user_whitespace_pass.as_str());
            if u.password_non_empty() {
                um.add_user(u);
            }
        }

        let mut opt_user_passes = option.user_passes.clone();
        if let Some(vu) = opt_user_passes.as_mut().filter(|vu| !vu.is_empty()) {
            while let Some(u) = vu.pop() {
                let uup = user_trait::PlainText::new(u.user, u.pass);
                um.add_user(uup);
            }
        }

        let mut oum: Option<UsersMap<PlainText>> = None;
        if !um.is_empty() {
            oum = Some(um);
        }

        Server {
            http_s: http_proxy::Server {
                um: oum.clone(),
                only_connect: false,
                ext_fields: Some(MapExtFields::default()),
            },
            socks5_s: socks5::server::Server {
                um: oum,
                support_udp: true, //默认打开udp 支持
                ext_fields: Some(MapExtFields::default()),
            },
            ext_fields: None,
        }
    }

    pub async fn handshake(
        &self,
        cid: CID,
        base: Conn,
        pre_read_data: Option<bytes::BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        let r = self
            .socks5_s
            .handshake(cid.clone(), base, pre_read_data)
            .await?;

        if let Some(e) = &r.e {
            debug!(cid = %cid, "socks5http error: {:#}", e);

            if r.b.is_some() {
                let c = match r.c {
                    Stream::Conn(c) => c,

                    _ => unimplemented!(),
                };
                debug!(cid = %cid, "trying http proxy  ",);

                let rr = self.http_s.handshake(cid, c, r.b).await?;

                return Ok(rr);
            }
        }
        Ok(r)
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
            _ => MapResult::from_err_str("socks5http only support tcplike stream"),
        }
    }
}
