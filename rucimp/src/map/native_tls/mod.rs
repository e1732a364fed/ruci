use std::{fmt, fs::File, io::Read};

use anyhow::Context;
use async_trait::async_trait;
use bytes::BytesMut;
use ruci::{
    map::{self, MapResult, MapperExtFields, ProxyBehavior},
    net::{self, helpers::EarlyDataWrapper, CID},
    Name,
};

use macro_mapper::*;
use tokio_native_tls::{native_tls::Identity, TlsAcceptor, TlsConnector};
use tracing::debug;

pub fn load(cert_path: &str, key_path: &str) -> anyhow::Result<Identity> {
    let mut cert_file = File::open(cert_path)?;
    let mut certs = vec![];
    cert_file
        .read_to_end(&mut certs)
        .context("cert_file read failed")?;

    let mut key_file = File::open(key_path)?;
    let mut key = vec![];
    key_file.read_to_end(&mut key)?;
    let pkcs8 = Identity::from_pkcs8(&certs, &key).context("Identity::from_pkcs8 failed")?;

    Ok(pkcs8)
}

#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub cert_f_path: String,
    pub key_f_path: String,
}
impl ServerOptions {
    pub fn get_server(&self) -> anyhow::Result<Server> {
        let id = load(&self.cert_f_path, &self.key_f_path).context("load cert or key failed")?;
        Ok(Server {
            ta: TlsAcceptor::from(
                tokio_native_tls::native_tls::TlsAcceptor::new(id)
                    .context("TlsAcceptor new failed")?,
            ),
            ext_fields: Some(MapperExtFields::default()),
        })
    }
}

#[mapper_ext_fields]
#[derive(Clone, MapperExt)]
pub struct Server {
    ta: TlsAcceptor,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rucimp::native_tls::Server,")
    }
}

impl Name for Server {
    fn name(&self) -> &'static str {
        "native_tls_server"
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
            debug!("tls server got pre_read_data, init with EarlyDataWrapper");
            let nc = EarlyDataWrapper::from(pre_read_data, conn);

            conn = Box::new(nc);
        }

        let c = self.ta.accept(conn).await?;

        Ok(MapResult::new_c(Box::new(c)).a(a).build())
    }
}

#[async_trait]
impl map::Mapper for Server {
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
                Err(e) => MapResult::from_e(e.context("TLS server handshake failed")),
            }
        } else {
            MapResult::err_str("tls only support tcplike stream")
        }
    }
}

#[mapper_ext_fields]
#[derive(Clone, Debug, MapperExt)]
pub struct Client {
    pub domain: String,
    pub in_secure: bool,
}

impl Name for Client {
    fn name(&self) -> &'static str {
        "native_tls_client"
    }
}

#[async_trait]
impl map::Mapper for Client {
    async fn maps(
        &self,
        _cid: CID,
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        let conn = params.c;
        if let ruci::net::Stream::Conn(conn) = conn {
            let connector = if self.in_secure {
                TlsConnector::from(
                    tokio_native_tls::native_tls::TlsConnector::builder()
                        .danger_accept_invalid_certs(true)
                        .danger_accept_invalid_hostnames(true)
                        .build()
                        .unwrap(),
                )
            } else {
                TlsConnector::from(tokio_native_tls::native_tls::TlsConnector::new().unwrap())
            };

            let r = connector.connect(&self.domain, conn).await;
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
            MapResult::err_str("tls only support tcplike stream")
        }
    }
}
