use async_trait::async_trait;
use bytes::BytesMut;
use http::{Request, Response, StatusCode};
use macro_mapper::NoMapperExt;
use ruci::{
    map::{self, MapResult, Mapper, ProxyBehavior},
    net::{self, helpers::EarlyDataWrapper, http::CommonConfig},
};
use tokio::sync::mpsc;

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
        //这是一个 h2 包规定的奇怪用法
        tokio::spawn(async move {
            connection.await.expect("connection failed");
            debug!(cid = %cc, "h2 await end");
        });

        let mut request = Request::builder().body(()).unwrap();

        let (resp, send_stream) = send_request.send_request(request, false)?;

        let recv_stream = resp.await?.into_body();

        let stream = super::H2Stream::new(recv_stream, send_stream);

        let mb = MapResult::new_c(Box::new(stream))
            .a(a)
            .b(early_data)
            .build();
        (Ok(mb))
    }
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
pub struct MainClient {}
impl ruci::Name for MainClient {
    fn name(&self) -> &str {
        "h2_main_client"
    }
}

impl MainClient {
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
        //这是一个 h2 包规定的奇怪用法
        tokio::spawn(async move {
            connection.await.expect("connection failed");
            debug!(cid = %cc, "h2 await end");
        });

        unimplemented!()
    }
}

#[async_trait]
impl Mapper for MainClient {
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
                Err(e) => MapResult::from_e(e.context("h2_main_client handshake failed")),
            }
        } else {
            MapResult::err_str("h2_main_client only support tcplike stream")
        }
    }
}
