/*!
 * 先试 socks5, 不是 socks5协议的话, 再试 http proxy
 */

use futures::executor::block_on;
use log::debug;
use macro_mapper::DefaultMapperExt;
use map::Stream;

use crate::map::{self, MapResult};
use crate::net::CID;
use crate::user::{self};
use crate::{
    net::{self, Conn},
    user::{PlainText, UsersMap},
    Name,
};

use super::{http, socks5, MapperBox, ToMapper};

#[derive(Default, Clone)]
pub struct Config {
    pub user_whitespace_pass: Option<String>,
    pub user_passes: Option<Vec<PlainText>>,
}

impl ToMapper for Config {
    fn to_mapper(&self) -> MapperBox {
        let a = block_on(Server::new(self.clone()));
        Box::new(a)
    }
}

#[derive(Debug, Clone, DefaultMapperExt)]
pub struct Server {
    pub http_s: http::Server,
    pub socks5_s: socks5::server::Server,
}

impl Name for Server {
    fn name(&self) -> &'static str {
        "socks5http_server"
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

        let mut opt_userpasses = option.user_passes.clone();
        if let Some(vu) = opt_userpasses.as_mut().filter(|vu| !vu.is_empty()) {
            while let Some(u) = vu.pop() {
                let uup = user::PlainText::new(u.user, u.pass);
                um.add_user(uup).await;
            }
        }

        let mut oum: Option<UsersMap<PlainText>> = None;
        if um.len().await > 0 {
            oum = Some(um);
        }

        Server {
            http_s: http::Server {
                um: oum.clone(),
                only_connect: false,
            },
            socks5_s: socks5::server::Server {
                um: oum,
                support_udp: false,
            },
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
            debug!("{cid} debug socks5http e, {}", e);

            if r.b.is_some() {
                let c = match r.c {
                    Stream::TCP(c) => c,

                    _ => unimplemented!(),
                };
                debug!("{cid} try http proxy  ",);

                let rr = self.http_s.handshake(cid, c, r.b).await?;

                return Ok(rr);
            }
        }
        Ok(r)
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
            _ => MapResult::err_str("socks5http only support tcplike stream"),
        }
    }
}
