use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use bytes::BytesMut;
use h2::client::{Connection, SendRequest};
use http::Request;
use macro_mapper::NoMapperExt;
use ruci::{
    map::{self, MapResult, Mapper, ProxyBehavior},
    net::{self, http::CommonConfig, Conn, CID},
};
use tokio::sync::Mutex;

use tracing::debug;

use super::*;

/// SingleClient 不使用 h2 的多路复用特性
#[derive(Clone, Debug, NoMapperExt, Default)]
pub struct SingleClient {}
impl ruci::Name for SingleClient {
    fn name(&self) -> &str {
        "h2_single_client"
    }
}

impl SingleClient {
    async fn handshake(
        &self,
        cid: net::CID,
        conn: net::Conn,
        a: Option<net::Addr>,
        early_data: Option<BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        let r = h2::client::handshake(conn).await;
        let (mut send_request, connection) = match r {
            Ok(r) => r,
            Err(e) => {
                let e = anyhow::anyhow!("accept h2 got e {}", e);
                return Ok(MapResult::from_e(e));
            }
        };

        //let cc = cid.clone();

        let stream =
            new_stream_by_send_request(cid, false, &mut send_request, None, Some(connection))
                .await?;

        let m = MapResult::new_c(Box::new(stream))
            .a(a)
            .b(early_data)
            .build();
        Ok(m)
    }
}

async fn new_stream_by_send_request(
    cid: CID,
    is_grpc: bool,
    send_request: &mut SendRequest<Bytes>,
    req: Option<Request<()>>,
    connection: Option<Connection<Conn>>,
) -> anyhow::Result<net::Conn> {
    //这是 h2 包规定的奇怪用法
    // todo: add a rx parameter and select it to impl graceful shutdown

    if let Some(connection) = connection {
        tokio::spawn(async move {
            connection.await.expect("connection failed");
            debug!(cid = %cid, "h2 await end");
        });
    }

    let (resp, send_stream) = match req {
        Some(r) => send_request.send_request(r, false)?,
        None => send_request.send_request(Request::builder().body(()).unwrap(), false)?,
    };

    let recv_stream = resp.await?.into_body();

    let stream: net::Conn = if is_grpc {
        Box::new(super::grpc::Stream::new(recv_stream, send_stream))
    } else {
        Box::new(super::H2Stream::new(recv_stream, send_stream))
    };

    Ok(stream)
}

#[async_trait]
impl Mapper for SingleClient {
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
                Err(e) => MapResult::from_e(e.context("h2_single_client handshake failed")),
            }
        } else {
            MapResult::err_str("h2_single_client only support tcplike stream")
        }
    }
}

/// MuxClient 使用 h2 的多路复用特性
#[derive(Clone, Debug, NoMapperExt, Default)]
pub struct MuxClient {
    pub http_config: Option<CommonConfig>,
    pub is_grpc: Option<bool>,

    req: Option<Request<()>>,

    cache: Arc<Mutex<Option<SendRequest<Bytes>>>>,
}
impl ruci::Name for MuxClient {
    fn name(&self) -> &str {
        "h2_mux_client"
    }
}

impl MuxClient {
    pub fn new(http_config: Option<CommonConfig>) -> Self {
        Self {
            req: http_config
                .clone()
                .map(|c| crate::net::build_request_from(&c, "http://")),
            http_config,
            ..Default::default()
        }
    }

    async fn handshake(
        &self,
        cid: net::CID,
        conn: Option<net::Conn>,
        a: Option<net::Addr>,
        early_data: Option<BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        let is_grpc = self.is_grpc.unwrap_or_default();

        match conn {
            Some(conn) => {
                //debug!("h2_mux_client got some conn");
                let r = h2::client::handshake(conn).await;
                let r = match r {
                    Ok(r) => r,
                    Err(e) => {
                        let e = anyhow::anyhow!("accept h2 got e {}", e);
                        return Ok(MapResult::from_e(e));
                    }
                };
                let (mut send_request, connection) = r;

                //let cc = cid.clone();

                let stream = new_stream_by_send_request(
                    cid,
                    is_grpc,
                    &mut send_request,
                    self.req.clone(),
                    Some(connection),
                )
                .await?;

                let m = MapResult::new_c(stream).a(a).b(early_data).build();

                *self.cache.lock().await = Some(send_request);

                Ok(m)
            }
            None => {
                //debug!(cid = %cid , "h2_mux_client got no conn");

                let mut sr = self.cache.lock().await;

                let mr = if sr.is_some() {
                    let mut real_r = sr.take().unwrap();
                    //debug!(cid = %cid , "h2_mux_client try to get new sub conn");

                    let cc = cid.clone();

                    let rrr = &mut real_r;
                    let stream =
                        new_stream_by_send_request(cc, is_grpc, rrr, self.req.clone(), None).await;
                    let stream = match stream {
                        Ok(s) => s,
                        Err(e) => {
                            debug!(cid = %cid , "h2_mux_client can't get sub stream,{e}");
                            return Err(e);
                        }
                    };
                    *sr = Some(real_r);
                    //debug!(cid = %cid , "h2_mux_client got new sub conn");

                    let m = MapResult::new_c(Box::new(stream))
                        .a(a)
                        .b(early_data)
                        .build();

                    Some(m)
                } else {
                    None
                };
                mr.ok_or(anyhow!("h2 not established yet"))
            }
        }
    }
}

#[async_trait]
impl Mapper for MuxClient {
    async fn maps(
        &self,
        cid: net::CID,
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        let conn = params.c;
        match conn {
            net::Stream::Conn(conn) => {
                let r = self.handshake(cid, Some(conn), params.a, params.b).await;
                match r {
                    anyhow::Result::Ok(r) => r,
                    Err(e) => MapResult::from_e(e.context("h2_mux_client handshake failed")),
                }
            }
            net::Stream::None => {
                let r = self.handshake(cid, None, params.a, params.b).await;
                match r {
                    anyhow::Result::Ok(r) => r,
                    Err(e) => MapResult::from_e(e.context("h2_mux_client handshake failed")),
                }
            }
            _ => MapResult::err_str("h2_mux_client only support tcplike stream or None stream"),
        }
    }
}
