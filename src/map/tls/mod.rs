/*
使用 async_tls(其使用了 rustls)
 */
mod load;

#[cfg(test)]
mod test;

use async_trait::async_trait;
use bytes::BytesMut;
use futures::AsyncWriteExt;
use log::debug;
use rustls::{client::ServerCertVerifier, ClientConfig, OwnedTrustAnchor};
use std::{fmt, sync::Arc};

use crate::{
    map,
    net::{self, helpers::EarlyDataWrapper},
};
use std::path::PathBuf;

use super::{MapResult, ProxyBehavior};

#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub addr: String,
    pub cert: PathBuf,
    pub key: PathBuf,
}

// todo: 添加 alpn 和 tls_minv
pub struct Server {
    pub option_cache: ServerOptions,
    ta: TlsAcceptor,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ruci::tls::ServerAdder, {:?}", self.option_cache)
    }
}

impl Server {
    pub fn new(c: ServerOptions) -> Self {
        let config = load::load_ser_config(&c).unwrap();
        Server {
            ta: TlsAcceptor::from(Arc::new(config)),
            option_cache: c.clone(),
        }
    }

    async fn handshake(
        &self,
        _cid: u32,
        mut conn: net::Conn,
        b: Option<BytesMut>,
        a: Option<net::Addr>,
    ) -> io::Result<map::MapResult> {
        if let Some(pre_read_data) = b {
            let nc = EarlyDataWrapper::from(pre_read_data, conn);

            conn = Box::new(nc);
        }

        let c = self.ta.accept(conn).await?;

        Ok(MapResult {
            a,
            b: None,
            c: Some(Box::new(c)),
            d: Some(map::AnyData::B(Box::new(SeverTLSConnDescriber {}))),
            e: None,
        })
    }
}

pub struct SeverTLSConnDescriber {}

#[async_trait]
impl map::Mapper for Server {
    fn name(&self) -> &'static str {
        "tls"
    }

    //behavior is always decode
    async fn maps(
        &self,
        cid: u32,
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        let conn = params.c;
        if let map::Stream::TCP(conn) = conn {
            let r = self.handshake(cid, conn, params.b, params.a).await;
            match r {
                Ok(r) => r,
                Err(e) => MapResult::from_err(e),
            }
        } else {
            MapResult::err_str("tls only support tcplike stream")
        }
    }
}

#[derive(Debug)]
pub struct Client {
    pub domain: String,
    pub is_insecure: bool,
    client_config: Arc<ClientConfig>,
}

impl Client {
    pub fn new(domain: &str, is_insecure: bool) -> Self {
        let mut root_certs = rustls::RootCertStore::empty();
        root_certs.add_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
            OwnedTrustAnchor::from_subject_spki_name_constraints(
                ta.subject,
                ta.spki,
                ta.name_constraints,
            )
        }));
        let mut config = ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_certs)
            .with_no_client_auth();

        //let server_name = rustls::ServerName::try_from(domain).unwrap();

        config
            .dangerous()
            //.set_certificate_verifier(Arc::new(SuperDanVer { domain: server_name }));
            .set_certificate_verifier(Arc::new(SuperDanVer {}));

        Client {
            domain: domain.to_string(),
            is_insecure,
            client_config: Arc::new(config),
        }
    }
}

/// only checks domain
struct SuperDanVer {
    //domain: rustls::ServerName,
}

impl ServerCertVerifier for SuperDanVer {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        //debug!("superdanver called, {:?}",server_name);
        debug!("superdanver called");
        //if !server_name.eq(&self.domain) {}//server_name是client自己提供的，
        //在不验证cert的情况下，没有必要和自己比较

        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

pub struct ClientTLSConnDescriber {}

impl Client {
    async fn handshake(
        &self,
        _cid: u32,
        conn: net::Conn,
        b: Option<BytesMut>,
        a: Option<net::Addr>,
    ) -> io::Result<MapResult> {
        let connector = if self.is_insecure {
            TlsConnector::from(self.client_config.clone())
        } else {
            TlsConnector::default()
        };

        let mut new_c = connector.connect(&self.domain, conn).await?;

        if let Some(ed) = b {
            new_c.write_all(&ed).await?;
        }

        Ok(MapResult {
            a,
            b: None,
            c: Some(Box::new(new_c)),
            d: Some(map::AnyData::B(Box::new(ClientTLSConnDescriber {}))),
            e: None,
        })
    }
}
#[async_trait]
impl map::Mapper for Client {
    fn name(&self) -> &'static str {
        "tls"
    }

    // behavior is always encode
    async fn maps(
        &self,
        cid: u32, //state 的 id
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        let conn = params.c;
        if let map::Stream::TCP(conn) = conn {
            let r = self.handshake(cid, conn, params.b, params.a).await;
            match r {
                Ok(r) => r,
                Err(e) => MapResult::from_err(e),
            }
        } else {
            MapResult::err_str("tls only support tcplike stream")
        }
    }
}
