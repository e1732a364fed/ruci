use async_trait::async_trait;
use bytes::BytesMut;
use macro_mapper::NoMapperExt;
use ruci::{
    map::{self, MapResult, Mapper, ProxyBehavior},
    net::{self, http::CommonConfig},
};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        handshake::server::{ErrorResponse, Request, Response},
        http::{HeaderValue, StatusCode},
    },
};
use tracing::{debug, warn};

use super::*;

#[derive(Clone, Debug, NoMapperExt, Default)]
pub struct Server {
    pub config: Option<CommonConfig>,
}

impl ruci::Name for Server {
    fn name(&self) -> &str {
        "websocket_server"
    }
}

use lazy_static::lazy_static;
lazy_static! {
    pub static ref EMPTY_HV: HeaderValue = HeaderValue::from_static("");
}

impl Server {
    async fn handshake(
        &self,
        cid: net::CID,
        conn: net::Conn,
        a: Option<net::Addr>,
    ) -> anyhow::Result<map::MapResult> {
        let mut early_data: Option<BytesMut> = None;

        let func = |r: &Request, response: Response| -> Result<Response, ErrorResponse> {
            use http::Response;

            if let Some(c) = &self.config {
                let given_host = r
                    .headers()
                    .get("Host")
                    .unwrap_or(&EMPTY_HV)
                    .to_str()
                    .expect("ok");

                if c.host != given_host {
                    let r: Response<Option<String>> = Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(None)
                        .expect("ok");

                    warn!(
                        cid = %cid,
                        given = given_host,
                        expected = c.host,
                        "ws server got wrong host"
                    );
                    return Err(r);
                }

                let given_path = r.uri().path();
                if c.path != given_path {
                    let r: Response<Option<String>> = Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(None)
                        .expect("ok");

                    warn!(
                        cid = %cid,
                        given = given_path,
                        expected = c.path,
                        "ws server got wrong path"
                    );
                    return Err(r);
                }
            }

            let ed_h = r.headers().get(EARLY_DATA_HEADER_KEY);

            if let Some(h) = ed_h {
                if h.len() > MAX_EARLY_DATA_LEN_BASE64 {
                    warn!(
                        "ws server got early data too long, won't decode at all: {}",
                        h.len()
                    );
                } else {
                    let given_early_data = h.to_str().expect("ok");

                    if !given_early_data.is_empty() {
                        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

                        let r = URL_SAFE_NO_PAD.decode(given_early_data);
                        match r {
                            Ok(v) => {
                                debug!("ws got early data {}", v.len());
                                early_data = Some(BytesMut::from(v.as_slice()))
                            }
                            Err(e) => {
                                warn!(
                                "ws server decode early data from {EARLY_DATA_HEADER_KEY} failed: {e}"
                            )
                            }
                        }
                    }
                }
            }

            Ok(response)
        };

        let c = accept_hdr_async(conn, func).await?;

        Ok(MapResult::new_c(Box::new(WsStreamToConnWrapper {
            ws: Box::pin(c),
            r_buf: None,
            w_buf: None,
        }))
        .a(a)
        .b(early_data)
        .build())
    }
}

#[async_trait]
impl Mapper for Server {
    async fn maps(
        &self,
        cid: net::CID,
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        let conn = params.c;
        if let net::Stream::Conn(conn) = conn {
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
