use anyhow::Context;
use async_trait::async_trait;
use ruci::{
    map::{self, *},
    net::{self, *},
};
use tokio_tungstenite::accept_async;

use super::WsStreamToConnWrapper;

#[derive(Clone, Debug)]
pub struct Server {}

impl ruci::Name for Server {
    fn name(&self) -> &str {
        "websocket_server"
    }
}

impl Server {
    async fn handshake(
        &self,
        _cid: CID,
        conn: net::Conn,
        a: Option<net::Addr>,
    ) -> anyhow::Result<map::MapResult> {
        let c = accept_async(conn)
            .await
            .with_context(|| "websocket server handshake failed")?;

        Ok(MapResult::newc(Box::new(WsStreamToConnWrapper {
            ws: c,
            r_buf: None,
            w_buf: None,
        }))
        .a(a)
        .build())
    }
}

#[async_trait]
impl Mapper for Server {
    async fn maps(
        &self,
        cid: CID,
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        let conn = params.c;
        if let Stream::Conn(conn) = conn {
            let r = self.handshake(cid, conn, params.a).await;
            match r {
                anyhow::Result::Ok(r) => r,
                Err(e) => MapResult::from_e(e.context("websocket_server handshake failed")),
            }
        } else {
            MapResult::err_str("websocket_server only support tcplike stream")
        }
    }
}
