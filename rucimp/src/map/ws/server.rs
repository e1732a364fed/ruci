use async_trait::async_trait;
use bytes::BytesMut;
use macro_map::*;
use ruci::{
    map::{self, Map, MapResult, ProxyBehavior},
    net::{self, helpers::EarlyDataWrapper, http::CommonConfig},
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

#[map_ext_fields]
#[derive(Clone, Debug, MapExt, Default)]
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
        mut conn: net::Conn,
        a: Option<net::Addr>,
        early_data: Option<BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        let mut real_early_data: Option<BytesMut> = None;

        let func = |r: &Request, response: Response| -> Result<Response, ErrorResponse> {
            use http::Response;

            if let Some(c) = &self.config {
                let r = crate::net::http::match_request_http_header(c, r);

                if let Err(e) = r {
                    let r: Response<Option<String>> = Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(None)
                        .expect("ok");

                    warn!(
                        cid = %cid,
                        e= %e,
                        "ws server got wrong http header"
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
                                debug!("ws server got early data {}", v.len());
                                real_early_data = Some(BytesMut::from(v.as_slice()))
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
        if let Some(b) = early_data {
            if !b.is_empty() {
                //debug!("wrap with earlydata_conn, {}", b.len());
                conn = Box::new(EarlyDataWrapper::from(b, conn));
            }
        }

        let c = accept_hdr_async(conn, func).await?;

        Ok(MapResult::new_c(Box::new(WsStreamToConnWrapper {
            ws: Box::pin(c),
            r_buf: None,
            w_buf: None,
        }))
        .a(a)
        .b(real_early_data)
        .build())
    }
}

#[async_trait]
impl Map for Server {
    async fn maps(
        &self,
        cid: net::CID,
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        let conn = params.c;
        if let net::Stream::Conn(conn) = conn {
            let r = self.handshake(cid, conn, params.a, params.b).await;
            match r {
                anyhow::Result::Ok(r) => r,
                Err(e) => MapResult::from_e(e.context("websocket_server handshake failed")),
            }
        } else {
            MapResult::err_str("websocket_server only support tcplike stream")
        }
    }
}
