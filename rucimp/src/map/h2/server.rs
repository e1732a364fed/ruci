use ::h2::server;
use anyhow::Context;
use async_trait::async_trait;
use bytes::BytesMut;
use http::{header::CONTENT_TYPE, Response, StatusCode};
use macro_mapper::NoMapperExt;
use ruci::{
    map::{self, MapResult, Mapper, ProxyBehavior},
    net::{self, helpers::EarlyDataWrapper, http::CommonConfig},
};
use tokio::sync::mpsc;

use tracing::warn;

use crate::map::h2::grpc::GRPC_CONTENT_TYPE;

use super::*;
#[derive(Clone, Debug, NoMapperExt, Default)]
pub struct Server {
    pub is_grpc: Option<bool>,

    pub http_config: Option<CommonConfig>,
}
impl ruci::Name for Server {
    fn name(&self) -> &str {
        "h2_server"
    }
}

impl Server {
    pub fn new(is_grpc: Option<bool>, http_config: Option<CommonConfig>) -> Self {
        Server {
            http_config,
            is_grpc,
        }
    }
    async fn handshake(
        &self,
        cid: net::CID,
        mut conn: net::Conn,
        early_data: Option<BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        if let Some(b) = early_data {
            if !b.is_empty() {
                //debug!(cid = %cid, "h2 wrap with earlydata_conn, {}", b.len());
                conn = Box::new(EarlyDataWrapper::from(b, conn));
            }
        }
        let mut conn = server::handshake(conn)
            .await
            .context("h2::server handshake failed")?;

        let (tx, rx) = mpsc::channel(100);

        let http_config = self.http_config.clone();
        let is_grpc = self.is_grpc.unwrap_or_default();

        tokio::spawn(async move {
            loop {
                //debug!(cid = %cid, "h2 server try accept");
                let r = conn.accept().await;
                //debug!(cid = %cid, "h2 server accepted");

                let r = match r {
                    Some(r) => r,
                    None => {
                        // 如果客户端发来的请求uri不带正确的 authority, h2
                        // 会在debug 中报 malformed headers: malformed authority
                        // 并对 accept 返回 None
                        warn!(cid = %cid, "accept h2 got none");
                        break;
                    }
                };
                let r = match r {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(cid = %cid, "accept h2 got e {}", e);
                        break;
                    }
                };
                let (req, mut resp) = r;

                if let Some(c) = &http_config {
                    //debug!(cid = %cid, "h2 req is {:?}",req);

                    if is_grpc {
                        let r = grpc::match_grpc_request_header(&req);

                        if let Err(e) = r {
                            warn!(
                                cid = %cid,
                                e= %e,
                                "h2 grpc server got wrong grpc header"
                            );
                            let _ = resp.send_response(
                                Response::builder()
                                    .status(StatusCode::BAD_REQUEST)
                                    .body(())
                                    .unwrap(),
                                false,
                            );

                            continue;
                        }
                    }
                    let r = crate::net::match_request_http_header(c, &req);

                    if let Err(e) = r {
                        warn!(
                            cid = %cid,
                            e= %e,
                            "h2 server got wrong http header"
                        );
                        let _ = resp.send_response(
                            Response::builder()
                                .status(StatusCode::BAD_REQUEST)
                                .body(())
                                .unwrap(),
                            false,
                        );

                        continue;
                    }
                }

                let (_, recv) = req.into_parts();

                let send = resp
                    .send_response(
                        if is_grpc {
                            Response::builder()
                                .header(CONTENT_TYPE, GRPC_CONTENT_TYPE)
                                .status(StatusCode::OK)
                                .body(())
                                .unwrap()
                        } else {
                            Response::builder().status(StatusCode::OK).body(()).unwrap()
                        },
                        false,
                    )
                    .map_err(|e| Error::new(ErrorKind::Interrupted, e));

                let send = match send {
                    Ok(send) => send,
                    Err(e) => {
                        warn!(cid = %cid, "accept h2 got e2 {}", e);
                        break;
                    }
                };
                let subid = recv.stream_id().as_u32();
                //let subid2 = send.stream_id().as_u32();
                //debug!(cid = %cid, "accept h2 got new {}", subid);
                //assert_eq!(subid, subid2);

                let stream: net::Conn = if is_grpc {
                    Box::new(super::grpc::Stream::new(recv, send))
                } else {
                    Box::new(super::H2Stream::new(recv, send))
                };

                let mut ncid = cid.clone();
                ncid.push_num(subid);

                let m = MapResult::new_c(stream).new_id(ncid).build();
                let r = tx.send(m).await;
                if let Err(e) = r {
                    warn!(cid = %cid, "accept h2 got e3 {}", e);
                    break;
                }
            }
        });

        let mr = MapResult::builder().c(net::Stream::Generator(rx)).build();
        Ok(mr)
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
            let r = self.handshake(cid, conn, params.b).await;
            match r {
                anyhow::Result::Ok(r) => r,
                Err(e) => MapResult::from_e(e.context("h2_server handshake failed")),
            }
        } else {
            MapResult::err_str("h2_server only support tcplike stream")
        }
    }
}
