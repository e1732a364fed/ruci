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
#[derive(Clone, Debug, NoMapperExt, Default)]
pub struct Client {}
impl ruci::Name for Client {
    fn name(&self) -> &str {
        "h2_client"
    }
}

impl Client {
    async fn handshake(
        &self,
        cid: net::CID,
        mut conn: net::Conn,
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

        //这是一个 h2 包规定的奇怪用法
        tokio::spawn(async {
            connection.await.expect("connection failed");
            debug!("await ok");
        });

        let mut request = Request::builder().body(()).unwrap();

        let (resp, send_stream) = send_request.send_request(request, false)?;

        let recv_stream = resp.await?.into_body();

        let stream = super::H2Stream::new(recv_stream, send_stream);

        let mb = MapResult::new_c(Box::new(stream)).build();
        (Ok(mb))
    }
}

#[async_trait]
impl Mapper for Client {
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

//pub struct MainClientConn {}
