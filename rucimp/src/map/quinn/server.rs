use crate::utils::FileSource;
use anyhow::Context;
use quinn::{Endpoint, ServerConfig};

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use ruci::map::*;
use ruci::net::{helpers, CID};
// use ruci::Name;
use ruci::{map, net::Stream};

use macro_map::*;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::map::{quic_common, rustls21};

#[map_ext_fields]
#[derive(Debug, Clone, MapExt)]
pub struct Server {
    // tls_key_path: String,
    // tls_cert_path: String,
    listen_addr: String,
    pub alpn: Option<Vec<String>>,

    next_cid: Arc<AtomicU32>,

    cached_server_config: rustls::ServerConfig,
}

// impl Name for Server {
//     fn name(&self) -> &'static str {
//         "quic_server"
//     }
// }

impl Server {
    pub fn new(c: quic_common::ServerConfig, file_source: &FileSource) -> anyhow::Result<Self> {
        let tls_server_config = rustls21::sc(
            rustls21::ServerOptions {
                alpn: c.alpn.clone(),
                cert_path: c.cert_path.clone(),
                key_path: c.key_path.clone(),
            },
            file_source,
        )
        .context("rustls21::sc failed")?;

        Ok(Self {
            // tls_key_path: c.key_path,
            // tls_cert_path: c.cert_path,
            listen_addr: c.listen_addr,
            alpn: c.alpn,
            next_cid: Arc::new(AtomicU32::new(1)),
            ext_fields: Some(MapExtFields::default()),
            cached_server_config: tls_server_config,
        })
    }
    async fn start_listen(&self, cid: CID) -> anyhow::Result<map::MapResult> {
        let server_config = ServerConfig::with_crypto(Arc::new(self.cached_server_config.clone()));

        let endpoint = Endpoint::server(server_config, self.listen_addr.parse()?)?;

        let (tx, rx) = mpsc::channel(100); //todo adjust this

        debug!(cid = %cid , laddr= self.listen_addr.as_str(), "quic server started");

        let ncid = self.next_cid.clone();

        tokio::spawn(async move {
            while let Some(connecting) = endpoint.accept().await {
                tokio::spawn(Server::handle_sub_stream(
                    connecting,
                    cid.clone(),
                    ncid.clone(),
                    tx.clone(),
                ));
            }
        });

        let mr = MapResult::builder()
            .c(ruci::net::Stream::Generator(rx))
            .build();
        Ok(mr)
    }

    async fn handle_sub_stream(
        connecting: quinn::Connecting,
        mut new_cid: CID,
        a_ncid: Arc<AtomicU32>,
        tx: mpsc::Sender<MapResult>,
    ) {
        let connection = connecting.await;
        let connection = match connection {
            Ok(c) => c,
            Err(e) => {
                warn!(cid = %new_cid, e = %e, "quic server await the new connecting failed");
                return;
            }
        };

        let cidc = new_cid.clone();
        new_cid.push_num(a_ncid.fetch_add(1, Ordering::Relaxed));
        debug!(cid = %cidc, new_cid = %new_cid, raddr = ?connection.remote_address(), "quic server got new conn");

        let cc = new_cid.clone();
        let tx = tx.clone();

        let s_count: AtomicU32 = AtomicU32::new(1);

        while let Ok((se, re)) = connection.accept_bi().await {
            let mut new_cid = cc.clone();
            new_cid.push_num(s_count.fetch_add(1, Ordering::Relaxed));

            debug!(cid = %cc, new_cid = %new_cid, raddr = ?connection.remote_address(), "quic server conn got new sub stream");

            let stream = helpers::RWWrapper { w: se, r: re };

            let stream = Box::new(stream);

            let m = MapResult::new_c(stream).new_id(new_cid).build();
            let r = tx.send(m).await;
            if let Err(e) = r {
                warn!(cid = %cc, "quic send tx got error: {}", e);
                break;
            }
        }
    }
}

#[async_trait]
impl Map for Server {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let conn = params.c;
        if let Stream::None = conn {
            match self.start_listen(cid).await {
                anyhow::Result::Ok(r) => r,
                Err(e) => MapResult::from_e(e.context("quic_server maps failed")),
            }
        } else {
            MapResult::from_err_str("quic_server only support None stream")
        }
    }
}
