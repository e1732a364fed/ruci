use ::h2::server;
use async_trait::async_trait;
use bytes::BytesMut;
use http::{Response, StatusCode};
use macro_mapper::NoMapperExt;
use ruci::{
    map::{self, MapResult, Mapper, ProxyBehavior},
    net::{self, helpers::EarlyDataWrapper, http::CommonConfig},
};
use tokio::sync::mpsc;

use tracing::{debug, info};

use super::*;
#[derive(Clone, Debug, NoMapperExt, Default)]
pub struct Server {
    pub config: Option<CommonConfig>,
}
impl ruci::Name for Server {
    fn name(&self) -> &str {
        "h2_server"
    }
}

impl Server {
    async fn handshake(
        &self,
        cid: net::CID,
        mut conn: net::Conn,
        early_data: Option<BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        if let Some(b) = early_data {
            if !b.is_empty() {
                debug!(cid = %cid, "h2 wrap with earlydata_conn, {}", b.len());
                conn = Box::new(EarlyDataWrapper::from(b, conn));
            }
        }
        let mut conn = server::handshake(conn).await?;

        let (tx, rx) = mpsc::channel(100);
        tokio::spawn(async move {
            loop {
                debug!(cid = %cid, "h2 server try accept");
                let r = conn.accept().await;
                debug!(cid = %cid, "h2 server accepted");

                let r = match r {
                    Some(r) => r,
                    None => {
                        info!(cid = %cid, "accept h2 got none");
                        break;
                    }
                };
                let r = match r {
                    Ok(r) => r,
                    Err(e) => {
                        info!(cid = %cid, "accept h2 got e {}", e);
                        break;
                    }
                };
                let (req, mut resp) = r;
                let (_, recv) = req.into_parts();

                let send = resp
                    .send_response(
                        Response::builder().status(StatusCode::OK).body(()).unwrap(),
                        false,
                    )
                    .map_err(|e| Error::new(ErrorKind::Interrupted, e));

                let send = match send {
                    Ok(send) => send,
                    Err(e) => {
                        info!("cid = %cid, accept h2 got e2 {}", e);
                        break;
                    }
                };
                let subid = recv.stream_id().as_u32();
                let subid2 = send.stream_id().as_u32();
                info!("cid = %cid, accept h2 got new {} {}", subid, subid2);
                assert!(subid == subid2);

                let stream = super::H2Stream::new(recv, send);
                info!("cid = %cid, accept h2 got new");
                let mut ncid = cid.clone();
                ncid.push_num(subid);

                let m = MapResult::new_c(Box::new(stream)).new_id(ncid).build();
                let r = tx.send(m).await;
                if let Err(e) = r {
                    info!("accept h2 got e3 {}", e);
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
