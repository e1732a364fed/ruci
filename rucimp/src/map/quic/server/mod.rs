use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use ruci::map::*;
use ruci::net::CID;
use ruci::Name;
use ruci::{map, net::Stream};

use macro_map::*;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::map::quic_common::ServerConfig;

#[map_ext_fields]
#[derive(Debug, Clone, MapExt)]
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
    pub fn new(c: ServerConfig) -> Self {
        Self {
            tls_key_path: c.key_path,
            tls_cert_path: c.cert_path,
            listen_addr: c.listen_addr,
            alpn: c.alpn,
            a_next_cid: Arc::new(AtomicU32::new(1)),
            ext_fields: Some(MapExtFields::default()),
        }
    }
    async fn start_listen(&self, cid: CID) -> anyhow::Result<map::MapResult> {
        //builder() use default, default will use h3 as alpn
        let mut tls = s2n_quic_rustls::Server::builder().with_certificate(
            Path::new(self.tls_cert_path.as_str()),
            Path::new(self.tls_key_path.as_str()),
        )?;
        if let Some(a) = &self.alpn {
            tls = tls.with_application_protocols(a.into_iter())?;
        }
        let tls = tls.build()?;

        let mut server = s2n_quic::Server::builder()
            .with_tls(tls)?
            .with_io(self.listen_addr.as_str())?
            .start()
            .context("quic init server failed")?;

        let (tx, rx) = mpsc::channel(100); //todo adjust this

        let cidc = cid.clone();
        let a_ncid = self.a_next_cid.clone();
        tokio::spawn(async move {
            // 这里会比较慢, 因为 accept 是要等一个新连接完全建立 (1rtt)
            // 后才返回, 如果同时有多个新连接，就会排队

            // 对比 quinn, 它返回一个 Connecting, 就好很多

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

        debug!(cid = %cidc , laddr= self.listen_addr.as_str(), "quic server started");
        let mr = MapResult::builder()
            .c(ruci::net::Stream::Generator(rx))
            .build();
        Ok(mr)
    }
}

#[async_trait]
impl Map for Server {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let conn = params.c;
        if let Stream::None = conn {
            let r = self.start_listen(cid).await;
            match r {
                anyhow::Result::Ok(r) => r,
                Err(e) => MapResult::from_e(e.context("quic_server maps failed")),
            }
        } else {
            MapResult::err_str("quic_server only support None stream")
        }
    }
}
