use std::{fmt, fs::File, io::Read};

use async_trait::async_trait;
use bytes::BytesMut;
use ruci::{
    map::{self, MapResult, ProxyBehavior},
    net::{self, helpers::EarlyDataWrapper, NamedConn, CID},
    Name,
};

use macro_mapper::*;
use tokio_native_tls::{native_tls::Identity, TlsAcceptor, TlsStream};
use tracing::debug;

pub fn load(cert_path: &str, key_path: &str) -> anyhow::Result<Identity> {
    let mut cert_file = File::open(cert_path)?;
    let mut certs = vec![];
    cert_file.read_to_end(&mut certs)?;

    let mut key_file = File::open(key_path)?;
    let mut key = vec![];
    key_file.read_to_end(&mut key)?;
    let pkcs8 = Identity::from_pkcs8(&certs, &key)?;

    Ok(pkcs8)
}

#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub cert_f_path: String,
    pub key_f_path: String,
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

pub struct TlsStreamWrapper(TlsStream<Box<dyn NamedConn>>);

impl ruci::Name for TlsStreamWrapper {
    fn name(&self) -> &str {
        "tokio_native_tls_stream"
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

        // todo add SeverTLSConnDescriber as data
        // Ok(MapResult::new_c(Box::new(TlsStreamWrapper(c))).a(a).build())
        unimplemented!()
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
