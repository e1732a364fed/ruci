use quinn::Endpoint;

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::BytesMut;
use ruci::map::*;
use ruci::net::{helpers, CID};
use ruci::Name;
use ruci::{map, net::Stream};

use macro_mapper::*;
use tokio::sync::Mutex;
use tracing::debug;

use crate::map::{quic_common, rustls21};

#[mapper_ext_fields]
#[derive(Debug, Clone, MapperExt)]
pub struct Client {
    c: Endpoint,
    conn: Arc<Mutex<Option<quinn::Connection>>>,

    server_addr: SocketAddr,
    server_name: String,
}

impl Name for Client {
    fn name(&self) -> &'static str {
        "quic_client"
    }
}

impl Client {
    pub fn new(c: quic_common::ClientConfig) -> anyhow::Result<Self> {
        let cc = {
            let cc = rustls21::cc(rustls21::ClientOptions {
                is_insecure: c.is_insecure.unwrap_or_default(),
                alpn: c.alpn,
                cert_path: c.cert_path.clone(),
            })?;

            quinn::ClientConfig::new(Arc::new(cc))
        };
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(cc);

        let a: SocketAddr = c.server_addr.parse()?;

        Ok(Self {
            c: endpoint,
            conn: Arc::new(Mutex::new(None)),
            server_addr: a,
            server_name: c.server_name,
            ext_fields: Some(MapperExtFields::default()),
        })
    }
    async fn handshake(
        &self,
        cid: CID,
        a: Option<ruci::net::Addr>,
        b: Option<BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        let mut conn = self.conn.lock().await;

        loop {
            if conn.is_none() {
                let connection = self
                    .c
                    .connect(self.server_addr, self.server_name.as_str())?
                    .await?;
                //connection.keep_alive(true)?;

                *conn = Some(connection);
                debug!(cid = %cid, "inited new quic connection");
            } else {
                let real_conn = conn.take().unwrap();
                let stream_r = real_conn.open_bi().await;
                *conn = Some(real_conn);

                let (se, re) = stream_r?;
                let stream = helpers::RWWrapper { w: se, r: re };

                let c: ruci::net::Conn = Box::new(stream);

                return Ok(MapResult::new_c(c).a(a).b(b).build());
            }
        }
    }
}

#[async_trait]
impl Mapper for Client {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let conn = params.c;
        if let Stream::None = conn {
            let r = self.handshake(cid, params.a, params.b).await;
            match r {
                anyhow::Result::Ok(r) => r,
                Err(e) => MapResult::from_e(e.context("quic_client maps failed")),
            }
        } else {
            MapResult::err_str("quic_client only support None stream")
        }
    }
}
