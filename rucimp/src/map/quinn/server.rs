use quinn::{Endpoint, ServerConfig};

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use ruci::map::*;
use ruci::net::CID;
use ruci::Name;
use ruci::{map, net::Stream};

use macro_mapper::*;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::map::{quic_common, rustls21};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub key_path: String,
    pub cert_path: String,
    pub listen_addr: String,
    pub alpn: Option<Vec<String>>,
}

#[mapper_ext_fields]
#[derive(Debug, Clone, MapperExt)]
pub struct Server {
    tls_key_path: String,
    tls_cert_path: String,
    listen_addr: String,
    pub alpn: Option<Vec<String>>,

    a_next_cid: Arc<AtomicU32>,
}

impl Name for Server {
    fn name(&self) -> &'static str {
        "quic_server"
    }
}

impl Server {
    pub fn new(c: quic_common::ServerConfig) -> Self {
        Self {
            tls_key_path: c.key_path,
            tls_cert_path: c.cert_path,
            listen_addr: c.listen_addr,
            alpn: c.alpn,
            a_next_cid: Arc::new(AtomicU32::new(1)),
            ext_fields: Some(MapperExtFields::default()),
        }
    }
    async fn handshake(&self, cid: CID) -> anyhow::Result<map::MapResult> {
        let server_config = rustls21::sc(rustls21::ServerOptions {
            alpn: self.alpn.clone(),
            cert_path: self.tls_cert_path.clone(),
            key_path: self.tls_key_path.clone(),
        })?;
        let server_config = ServerConfig::with_crypto(Arc::new(server_config));

        let endpoint = Endpoint::server(server_config, self.listen_addr.parse()?)?;

        let (tx, rx) = mpsc::channel(100); //todo adjust this

        let cidc = cid.clone();
        let a_ncid = self.a_next_cid.clone();
        tokio::spawn(async move {
            let a_ncid = a_ncid.clone();
            let tx = tx.clone();
            while let Some(connecting) = endpoint.accept().await {
                let mut new_cid = cidc.clone();
                let cidc = cidc.clone();

                let a_ncid = a_ncid.clone();

                let tx = tx.clone();
                tokio::spawn(async move {
                    let connection = connecting.await;
                    let connection = match connection {
                        Ok(c) => c,
                        Err(e) => {
                            warn!(cid = %new_cid, e = %e, "quic server await the new connecting failed");
                            return;
                        }
                    };

                    new_cid.push_num(a_ncid.fetch_add(1, Ordering::Relaxed));
                    debug!(cid = %cidc, new_cid = %new_cid, raddr = ?connection.remote_address(), "quic server got new conn");

                    let cc = new_cid.clone();
                    let tx = tx.clone();

                    let s_count: AtomicU32 = AtomicU32::new(1);

                    while let Ok((se, re)) = connection.accept_bi().await {
                        let mut new_cid = cc.clone();
                        new_cid.push_num(s_count.fetch_add(1, Ordering::Relaxed));

                        debug!(cid = %cc, new_cid = %new_cid, raddr = ?connection.remote_address(), "quic server conn got new sub stream");

                        let stream = super::Stream::new(se, re);

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

        debug!(cid = %cid , laddr= self.listen_addr.as_str(), "quic server started");
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
