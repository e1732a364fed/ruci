use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::BytesMut;
use ruci::map::*;
use ruci::net::CID;
use ruci::Name;
use ruci::{map, net::Stream};

use macro_mapper::*;
use s2n_quic::client::Connect;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::debug;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub cert_path: String,

    pub server_addr: String,
    pub server_name: String,
    pub alpn: Option<Vec<String>>,
}

#[mapper_ext_fields]
#[derive(Debug, Clone, MapperExt)]
pub struct Client {
    c: s2n_quic::Client,
    conn: Arc<Mutex<Option<s2n_quic::Connection>>>,

    server_addr: SocketAddr,
    server_name: String,
}

impl Name for Client {
    fn name(&self) -> &'static str {
        "quic_client"
    }
}

impl Client {
    pub fn new(c: Config) -> anyhow::Result<Self> {
        let mut tls =
            s2n_quic_rustls::Client::builder().with_certificate(Path::new(c.cert_path.as_str()))?;

        if let Some(a) = c.alpn {
            tls = tls.with_application_protocols(a.into_iter())?;
        }

        let tls = tls.build()?;

        let client = s2n_quic::Client::builder()
            .with_tls(tls)?
            .with_io("0.0.0.0:0")?
            .start()?;

        let a: SocketAddr = c.server_addr.parse()?;

        Ok(Self {
            c: client,
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
                let connect =
                    Connect::new(self.server_addr).with_server_name(self.server_name.clone());
                let mut connection = self.c.connect(connect).await?;
                connection.keep_alive(true)?;

                *conn = Some(connection);
                debug!(cid = %cid, "inited new s2n_quic connection");
            } else {
                let mut real_conn = conn.take().unwrap();
                let stream_r = real_conn.open_bidirectional_stream().await;
                *conn = Some(real_conn);

                let stream = stream_r?;

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
