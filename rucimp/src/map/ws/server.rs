use async_trait::async_trait;
use macro_mapper::NoMapperExt;
use ruci::{
    map::{self, MapResult, Mapper, ProxyBehavior},
    net::{self, http::CommonConfig},
};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::http::{HeaderValue, StatusCode},
};
use tracing::warn;

use super::WsStreamToConnWrapper;

#[derive(Clone, Debug, NoMapperExt, Default)]
pub struct Server {
    pub config: Option<CommonConfig>,
}

impl ruci::Name for Server {
    fn name(&self) -> &str {
        "websocket_server"
    }
}

impl Server {
    async fn handshake(
        &self,
        cid: net::CID,
        conn: net::Conn,
        a: Option<net::Addr>,
    ) -> anyhow::Result<map::MapResult> {
        let func = |r: &tokio_tungstenite::tungstenite::handshake::server::Request,
                    response: tokio_tungstenite::tungstenite::handshake::server::Response|
         -> Result<
            tokio_tungstenite::tungstenite::handshake::server::Response,
            tokio_tungstenite::tungstenite::handshake::server::ErrorResponse,
        > {
            use http::Response;

            if let Some(c) = &self.config {
                let empty_hv = HeaderValue::from_static("");
                let given_host = r
                    .headers()
                    .get("Host")
                    .unwrap_or(&empty_hv)
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
                        "websocket server got wrong host"
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
                        "websocket server got wrong path"
                    );
                    return Err(r);
                }
            }
            Ok(response)
        };

        let c = accept_hdr_async(conn, func).await?;

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
