use std::io::{self};

use futures::executor::block_on;
use log::debug;

use crate::map::{self, MapResult};
use crate::net::CID;
use crate::user::{self};
use crate::{
    net::{self, Conn},
    user::{UserPass, UsersMap},
    Name,
};

use super::{http, socks5, MapperBox, ToMapper};

#[derive(Default, Clone)]
pub struct Config {
    pub user_whitespace_pass: Option<String>,
    pub user_passes: Option<Vec<UserPass>>,
}

impl ToMapper for Config {
    fn to_mapper(&self) -> MapperBox {
        let a = block_on(Server::new(self.clone()));
        Box::new(a)
    }
}

#[derive(Debug, Clone)]
pub struct Server {
    pub https: http::Server,
    pub socks5s: socks5::server::Server,
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
            let u = UserPass::from(user_whitespace_pass);
            if u.strict_valid() {
                um.add_user(u).await;
            }
        }

        let mut opt_userpasses = option.user_passes.clone();
        if let Some(vu) = opt_userpasses.as_mut().filter(|vu| !vu.is_empty()) {
            while let Some(u) = vu.pop() {
                let uup = user::UserPass::new(u.user, u.pass);
                um.add_user(uup).await;
            }
        }

        let mut oum: Option<UsersMap<UserPass>> = None;
        if um.len().await > 0 {
            oum = Some(um);
        }

        Server {
            https: http::Server {
                um: oum.clone(),
                only_connect: false,
            },
            socks5s: socks5::server::Server {
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
    ) -> io::Result<map::MapResult> {
        let r = self
            .socks5s
            .handshake(cid.clone(), base, pre_read_data)
            .await?;

        if r.e.is_some() {
            debug!("{cid} debug socks5http e, {}", r.e.as_ref().unwrap());

            if r.b.is_some() {
                let c = match r.c {
                    net::Stream::TCP(c) => c,

                    _ => unimplemented!(),
                };
                debug!("{cid} try https  ",);

                let rr = self.https.handshake(cid, c, r.b).await?;

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
