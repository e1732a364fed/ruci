use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use ruci::map::*;
use ruci::net::CID;
use ruci::Name;
use ruci::{map, net::Stream};

use macro_mapper::*;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, warn};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub key_path: String,
    pub cert_path: String,
    pub listen_addr: String,
}

#[mapper_ext_fields]
#[derive(Debug, Clone, MapperExt)]
pub struct Server {
    tls_key_path: String,
    tls_cert_path: String,
    listen_addr: String,

    a_next_cid: Arc<AtomicU32>,
}

impl Name for Server {
    fn name(&self) -> &'static str {
        "quic_server"
    }
}

impl Server {
    pub fn new(c: Config) -> Self {
        Self {
            tls_key_path: c.key_path,
            tls_cert_path: c.cert_path,
            listen_addr: c.listen_addr,
            a_next_cid: Arc::new(AtomicU32::new(1)),
            ext_fields: Some(MapperExtFields::default()),
        }
    }
    async fn handshake(&self, cid: CID) -> anyhow::Result<map::MapResult> {
        let mut server = s2n_quic::Server::builder()
            .with_tls((
                Path::new(self.tls_cert_path.as_str()),
                Path::new(self.tls_key_path.as_str()),
            ))?
            .with_io(self.listen_addr.as_str())?
            .start()
            .context("quic init server failed")?;

        let (tx, rx) = mpsc::channel(100); //todo adjust this

        let a_ncid = self.a_next_cid.clone();
        tokio::spawn(async move {
            while let Some(mut connection) = server.accept().await {
                let mut new_cid = cid.clone();
                new_cid.push_num(a_ncid.fetch_add(1, Ordering::Relaxed));
                debug!(cid = %cid, new_cid = %new_cid, raddr = ?connection.remote_addr(), "quic server got new conn");

                let cc = new_cid.clone();
                let tx = tx.clone();

                tokio::spawn(async move {
                    let s_count: AtomicU32 = AtomicU32::new(1);

                    while let Ok(Some(stream)) = connection.accept_bidirectional_stream().await {
                        let mut new_cid = cc.clone();
                        new_cid.push_num(s_count.fetch_add(1, Ordering::Relaxed));

                        debug!(cid = %cc, new_cid = %new_cid, raddr = ?connection.remote_addr(), "quic server conn got new sub stream");

                        let stream = Box::new(stream);

                        let m = MapResult::new_c(stream).new_id(new_cid).build();
                        let r = tx.send(m).await;
                        if let Err(e) = r {
                            warn!(cid = %cc, "quic send tx got error: {}", e);
                            break;
                        }
                    }
                });
            }
        });

        let mr = MapResult::builder()
            .c(ruci::net::Stream::Generator(rx))
            .build();
        Ok(mr)
    }
}

#[async_trait]
impl Mapper for Server {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let conn = params.c;
        if let Stream::None = conn {
            let r = self.handshake(cid).await;
            match r {
                anyhow::Result::Ok(r) => r,
                Err(e) => MapResult::from_e(e.context("quic_server maps failed")),
            }
        } else {
            MapResult::err_str("quic_server only support None stream")
        }
    }
}
