use anyhow::Context;
use async_trait::async_trait;
use bytes::BytesMut;
use macro_mapper::NoMapperExt;
use ruci::{
    map::{self, *},
    net::{self, http::CommonConfig, *},
};
use tokio_tungstenite::{
    client_async,
    tungstenite::http::{Request, StatusCode},
};

use super::WsStreamToConnWrapper;

#[derive(Clone, Debug, Default, NoMapperExt)]
pub struct Client {
    request: Request<()>,
}

impl ruci::Name for Client {
    fn name(&self) -> &str {
        "websocket_client"
    }
}

const EARLY_DATA_HEADER_K: &str = "k";
const EARLY_DATA_HEADER_V: &str = "v";

impl Client {
    pub fn new(c: CommonConfig) -> Self {
        let mut request = Request::builder()
            .method("GET")
            .header("Host", c.host.as_str())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .uri("ws://".to_string() + c.host.as_str() + &c.path);

        if let Some(h) = c.headers {
            for (k, v) in h.iter() {
                if k != "Host" {
                    request = request.header(k.as_str(), v.as_str());
                }
            }
        }

        if c.is_early_data.unwrap_or_default() {
            request = request.header(EARLY_DATA_HEADER_K, EARLY_DATA_HEADER_V);
        }
        let r = request.body(()).unwrap();
        Self { request: r }
    }
    async fn handshake(
        &self,
        _cid: CID,
        conn: net::Conn,
        a: Option<net::Addr>,
        b: Option<BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        let (c, resp) = client_async(self.request.clone(), conn)
            .await
            .with_context(|| "websocket client handshake failed")?;

        if resp.status() != StatusCode::SWITCHING_PROTOCOLS {
            return Err(anyhow::anyhow!(
                "websocket client handshake got resp status not SWITCHING_PROTOCOLS: {}",
                resp.status()
            ));
        }
        Ok(MapResult::newc(Box::new(WsStreamToConnWrapper {
            ws: Box::pin(c),
            r_buf: None,
            w_buf: b,
        }))
        .a(a)
        .build())
    }
}

#[async_trait]
impl Mapper for Client {
    async fn maps(
        &self,
        cid: CID,
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        let conn = params.c;
        if let Stream::Conn(conn) = conn {
            let r = self.handshake(cid, conn, params.a, params.b).await;
            match r {
                anyhow::Result::Ok(r) => r,
                Err(e) => MapResult::from_e(e.context("websocket_client maps failed")),
            }
        } else {
            MapResult::err_str("websocket_client only support tcplike stream")
        }
    }
}
