/*!
Defines [`ruci::map::Map`]s for TLS using `tokio_native_tls`.

 */

use std::{
    fmt::{self, Display},
    path::PathBuf,
};

use anyhow::Context;
use async_trait::async_trait;
use bytes::BytesMut;
use ruci::{
    map::{self, MapExtFields, MapResult, ProxyBehavior},
    net::{self, helpers::EarlyDataWrapper, CID},
};

use macro_map::*;
use tokio_native_tls::{native_tls::Identity, TlsAcceptor, TlsConnector};
use tracing::debug;

use data_source::DataSource;

pub fn load(cert_path: PathBuf, key_path: PathBuf, fs: &DataSource) -> anyhow::Result<Identity> {
    let cert_file = fs.read_to_string(cert_path)?;

    let key_file = fs.read_to_string(key_path)?;
    let pkcs8 = Identity::from_pkcs8(cert_file.as_bytes(), key_file.as_bytes())
        .context("Identity::from_pkcs8 failed")?;

    Ok(pkcs8)
}

impl Server {
    pub fn from(
        sc: &ruci_rustls22::server::TlsServerOptions,
        fs: &DataSource,
    ) -> anyhow::Result<Server> {
        let id = load(sc.cert.clone(), sc.key.clone(), fs).context("load cert or key failed")?;

        //native_tls 的 acceptor 的 builder 是不支持配置 alpn的，只有 connector 才支持
        let ta =
            tokio_native_tls::native_tls::TlsAcceptor::new(id).context("TlsAcceptor new failed")?;

        let ta = TlsAcceptor::from(ta);
        Ok(Server {
            ta,
            ext_fields: Some(MapExtFields::default()),
        })
    }
}

#[map_ext_fields]
#[derive(Clone, MapExt)]
pub struct Server {
    ta: TlsAcceptor,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rucimp::map::native_tls::Server")
    }
}

impl Display for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "native_tls_server")
    }
}

impl Server {
    async fn handshake(
        &self,
        _cid: CID,
        mut conn: net::Conn,
        b: Option<BytesMut>,
        a: Option<net::Addr>,
    ) -> anyhow::Result<map::MapResult> {
        if let Some(pre_read_data) = b {
            //debug!("tls server got pre_read_data, init with EarlyDataWrapper");
            let nc = EarlyDataWrapper::from(pre_read_data, conn);

            conn = Box::new(nc);
        }

        let c = self.ta.accept(conn).await?;

        Ok(MapResult::new_c(Box::new(c)).a(a).build())
    }
}

#[async_trait]
impl map::Map for Server {
    async fn maps(
        &self,
        cid: CID,
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        let conn = params.c;
        if let ruci::net::Stream::Conn(conn) = conn {
            let r = self.handshake(cid, conn, params.b, params.a).await;
            match r {
                anyhow::Result::Ok(r) => r,
                Err(e) => MapResult::from_e(e.context("NativeTLS server handshake failed")),
            }
        } else {
            MapResult::from_err_str("tls only support tcplike stream")
        }
    }
}

#[map_ext_fields]
#[derive(Clone, Debug, MapExt)]
pub struct Client {
    pub config: ruci::map::tls_config::ClientOptions,
}

impl Display for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "native_tls_client")
    }
}

#[async_trait]
impl map::Map for Client {
    async fn maps(
        &self,
        _cid: CID,
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        let conn = params.c;
        if let ruci::net::Stream::Conn(conn) = conn {
            let connector = if self.config.insecure {
                let mut b = tokio_native_tls::native_tls::TlsConnector::builder();

                if let Some(a) = &self.config.alpn {
                    let a: Vec<_> = a.iter().map(|s| s.as_str()).collect();
                    b.request_alpns(&a);
                }

                TlsConnector::from(
                    b.danger_accept_invalid_certs(true)
                        .danger_accept_invalid_hostnames(true)
                        .build()
                        .unwrap(),
                )
            } else {
                let mut b = tokio_native_tls::native_tls::TlsConnector::builder();
                if let Some(a) = &self.config.alpn {
                    let a: Vec<_> = a.iter().map(|s| s.as_str()).collect();
                    b.request_alpns(&a);
                }

                TlsConnector::from(b.build().unwrap())
            };

            if self.config.host.is_none() {
                debug!("host: {:?}", params.a);
            }
            let host = self.config.host.clone().map_or_else(
                || {
                    params
                        .a
                        .as_ref()
                        .and_then(|a| a.get_name_or_ip_string())
                        .unwrap_or_default()
                },
                |h| h,
            );

            let r = connector.connect(&host, conn).await;
            match r {
                anyhow::Result::Ok(c) => {
                    return MapResult::new_c(Box::new(c))
                        .a(params.a)
                        .b(params.b)
                        .build()
                }
                Err(e) => MapResult::from_e(e),
            }
        } else {
            MapResult::from_err_str("tls only support tcplike stream")
        }
    }
}
