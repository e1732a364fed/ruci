use std::sync::Arc;

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use bytes::BytesMut;
use h2::client::{Connection, SendRequest};
use http::Request;
use macro_mapper::NoMapperExt;
use ruci::{
    map::{self, MapResult, Mapper, ProxyBehavior},
    net::{self, http::CommonConfig, Conn, CID},
};
use tokio::sync::{
    oneshot::{self},
    Mutex,
};

use tracing::debug;

use super::*;

/// SingleClient 不使用 h2 的多路复用特性
#[derive(Clone, Debug, NoMapperExt, Default)]
pub struct SingleClient {
    pub http_config: Option<CommonConfig>,
    pub is_grpc: bool,

    req: Option<Request<()>>,
}
impl ruci::Name for SingleClient {
    fn name(&self) -> &str {
        "h2_single_client"
    }
}

impl SingleClient {
    pub fn new(is_grpc: bool, http_config: Option<CommonConfig>) -> Self {
        debug!("h2 new single client");

        let req = if is_grpc {
            http_config.as_ref().map(grpc::build_grpc_request_from)
        } else {
            http_config
                .as_ref()
                .map(|c| crate::net::build_request_from(c, "http://"))
        };

        Self {
            req,
            http_config,
            is_grpc,
        }
    }

    async fn handshake(
        &self,
        cid: net::CID,
        conn: net::Conn,
        a: Option<net::Addr>,
        early_data: Option<BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        //debug!(cid = %cid,"h2 single client handshake");

        let r = h2::client::handshake(conn).await;
        let (mut send_request, connection) = match r {
            Ok(r) => r,
            Err(e) => {
                let e = anyhow::anyhow!("h2 single client handshake got e {}", e);
                return Ok(MapResult::from_e(e));
            }
        };

        let stream = new_stream_by_send_request(
            cid,
            self.is_grpc,
            false,
            &mut send_request,
            self.req.clone(),
            Some(connection),
        )
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
    is_mux: bool,
    send_request: &mut SendRequest<Bytes>,
    req: Option<Request<()>>,
    connection: Option<Connection<Conn>>,
) -> anyhow::Result<net::Conn> {
    //这是 h2 包规定的奇怪用法, 必须 await Connection

    let (tx, rx) = oneshot::channel();

    if let Some(connection) = connection {
        tokio::spawn(async move {
            // 在连接被强制断开时，connection 就会 返回

            if !is_mux {
                tokio::select! {
                    r = connection=>{
                        debug!(cid = %cid, r=?r, "h2 stream disconnected");
                    }
                    _ = rx =>{
                        debug!(cid = %cid, "h2 stream got shutdown signal");
                    }
                }
            } else {
                let r = connection.await;
                debug!(cid = %cid, r=?r, "h2 stream disconnected");
            }
        });
    }

    let (resp, send_stream) = match req {
        Some(r) => send_request.send_request(r, false)?,
        None => send_request.send_request(Request::builder().body(()).unwrap(), false)?,
    };

    let recv_stream = resp.await?.into_body();

    let stream: net::Conn = if is_grpc {
        Box::new(super::grpc::Stream::new(
            recv_stream,
            send_stream,
            if !is_mux { Some(tx) } else { None },
        ))
    } else {
        Box::new(super::H2Stream::new(
            recv_stream,
            send_stream,
            if !is_mux { Some(tx) } else { None },
        ))
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
    pub is_grpc: bool,

    req: Option<Request<()>>,

    cache: Arc<Mutex<Option<SendRequest<Bytes>>>>,
}
impl ruci::Name for MuxClient {
    fn name(&self) -> &str {
        "h2_mux_client"
    }
}

impl MuxClient {
    pub fn new(is_grpc: bool, http_config: Option<CommonConfig>) -> Self {
        let req = if is_grpc {
            http_config.as_ref().map(grpc::build_grpc_request_from)
        } else {
            http_config
                .as_ref()
                .map(|c| crate::net::build_request_from(c, "http://"))
        };

        Self {
            req,
            http_config,
            is_grpc,
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
        let is_grpc = self.is_grpc;

        match conn {
            Some(conn) => {
                let mut cache = self.cache.lock().await;
                if cache.is_none() {
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

                    let stream = new_stream_by_send_request(
                        cid,
                        is_grpc,
                        true,
                        &mut send_request,
                        self.req.clone(),
                        Some(connection),
                    )
                    .await?;

                    let m = MapResult::new_c(stream).a(a).b(early_data).build();

                    *cache = Some(send_request);
                    Ok(m)
                } else {
                    bail!("can't handshake multiple times")
                }
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
                        new_stream_by_send_request(cc, is_grpc, true, rrr, self.req.clone(), None)
                            .await;
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
