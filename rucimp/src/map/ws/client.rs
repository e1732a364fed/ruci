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

use super::*;

#[derive(Clone, Debug, Default, NoMapperExt)]
pub struct Client {
    request: Request<()>,
    use_early_data: bool,
}

impl ruci::Name for Client {
    fn name(&self) -> &str {
        "websocket_client"
    }
}

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

        let r = request.body(()).unwrap();
        Self {
            request: r,
            use_early_data: c.use_early_data.unwrap_or_default(),
        }
    }
    async fn handshake(
        &self,
        _cid: CID,
        conn: net::Conn,
        a: Option<net::Addr>,
        b: Option<BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        let req = self.request.clone();
        if self.use_early_data {
            // if let Some(ref b) = b {
            //     debug!("will use earlydata {}", b.len());
            //     use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

            //     let str = URL_SAFE_NO_PAD.encode(b);
            //     req.headers_mut().insert(
            //         EARLY_DATA_HEADER_KEY,
            //         HeaderValue::from_str(&str).expect("ok"),
            //     );
            // }
        }

        let (c, resp) = client_async(req, conn)
            .await
            .with_context(|| "websocket client handshake failed")?;

        if resp.status() != StatusCode::SWITCHING_PROTOCOLS {
            return Err(anyhow::anyhow!(
                "websocket client handshake got resp status not SWITCHING_PROTOCOLS: {}",
                resp.status()
            ));
        }
        Ok(MapResult::new_c(Box::new(WsStreamToConnWrapper {
            ws: Box::pin(c),
            r_buf: None,
            w_buf: None,
        }))
        .a(a)
        .b(b)
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
