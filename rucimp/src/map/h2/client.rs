use std::{collections::HashSet, sync::Arc};

use anyhow::anyhow;
use async_trait::async_trait;
use bytes::BytesMut;
use futures::stream;
use h2::client::{Connection, SendRequest};
use http::{Request, Response, StatusCode};
use macro_mapper::NoMapperExt;
use ruci::{
    map::{self, MapResult, Mapper, ProxyBehavior},
    net::{self, helpers::EarlyDataWrapper, http::CommonConfig, Conn, CID},
};
use tokio::sync::{mpsc, Mutex};

use tracing::{debug, info};

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
        mut conn: net::Conn,
        a: Option<net::Addr>,
        early_data: Option<BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        let r = h2::client::handshake(conn).await;
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                let e = anyhow::anyhow!("accept h2 got e {}", e);
                return Ok(MapResult::from_e(e));
            }
        };
        let (mut send_request, connection) = r;

        let cc = cid.clone();

        let stream = new_stream_by_send_request(cc, &mut send_request, Some(connection)).await?;

        let m = MapResult::new_c(Box::new(stream))
            .a(a)
            .b(early_data)
            .build();
        (Ok(m))
    }
}

async fn new_stream_by_send_request(
    cid: CID,
    send_request: &mut SendRequest<Bytes>,
    connection: Option<Connection<Conn>>,
) -> anyhow::Result<H2Stream> {
    //这是 h2 包规定的奇怪用法
    // todo: add a rx parameter and select it to impl graceful shutdown

    if let Some(connection) = connection {
        tokio::spawn(async move {
            connection.await.expect("connection failed");
            debug!(cid = %cid, "h2 await end");
        });
    }

    let mut request = Request::builder().body(()).unwrap();

    let (resp, send_stream) = send_request.send_request(request, false)?;

    let recv_stream = resp.await?.into_body();

    let stream = super::H2Stream::new(recv_stream, send_stream);

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

#[derive(Clone, Debug, NoMapperExt, Default)]
pub struct MuxClient {
    cache: Arc<Mutex<Option<SendRequest<Bytes>>>>,
}
impl ruci::Name for MuxClient {
    fn name(&self) -> &str {
        "h2_mux_client"
    }
}

impl MuxClient {
    async fn handshake(
        &self,
        cid: net::CID,
        conn: Option<net::Conn>,
        a: Option<net::Addr>,
        early_data: Option<BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        match conn {
            Some(conn) => {
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

                let stream =
                    new_stream_by_send_request(cid, &mut send_request, Some(connection)).await?;

                let m = MapResult::new_c(Box::new(stream))
                    .a(a)
                    .b(early_data)
                    .build();

                *self.cache.lock().await = Some(send_request);

                Ok(m)
            }
            None => {
                let mut r = self.cache.lock().await;

                let rm = if r.is_some() {
                    let mut rm: Option<MapResult> = None;
                    r.as_mut().map(|mut r| {
                        let stream = futures::executor::block_on(async move {
                            new_stream_by_send_request(cid, r, None).await
                        });
                        let stream = match stream {
                            Ok(s) => s,
                            Err(_) => return,
                        };

                        let m = MapResult::new_c(Box::new(stream))
                            .a(a)
                            .b(early_data)
                            .build();

                        rm = Some(m)
                    });
                    rm
                } else {
                    None
                };
                rm.ok_or(anyhow!("h2 not established yet"))
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
                    Err(e) => MapResult::from_e(e.context("h2_main_client handshake failed")),
                }
            }
            net::Stream::None => {
                let r = self.handshake(cid, None, params.a, params.b).await;
                match r {
                    anyhow::Result::Ok(r) => r,
                    Err(e) => MapResult::from_e(e.context("h2_main_client handshake failed")),
                }
            }
            _ => MapResult::err_str("h2_main_client only support tcplike stream or None stream"),
        }
    }
}
