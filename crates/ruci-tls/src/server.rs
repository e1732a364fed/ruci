use std::{fmt::Display, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use bytes::BytesMut;
use macro_map::*;
use ruci::{
    map::{self, MapResult, ProxyBehavior},
    net::{helpers::EarlyDataWrapper, CID},
};
use serde::{Deserialize, Serialize};

use ruci::map::MapExtFields;
use tokio_rustls::TlsAcceptor;
use tracing::debug;

use super::*;

#[derive(Debug, Clone, Default)]
pub struct ServerPEMOptions {
    pub cert: String,
    pub key: String,
    pub alpn: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TlsServerOptions {
    pub cert: PathBuf,
    pub key: PathBuf,
    pub alpn: Option<Vec<String>>,
}

impl From<ServerPEMOptions> for map::MapBox {
    fn from(sc: ServerPEMOptions) -> Self {
        Box::new(Server::new(sc))
    }
}

// todo: 添加  tls_min_v
#[map_ext_fields]
#[derive(Clone, MapExt)]
pub struct Server {
    pub option_cache: ServerPEMOptions,
    ta: TlsAcceptor,
}

impl Display for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "ruci::tls::Server")
    }
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "ruci::tls::Server, {:?}", self.option_cache)
    }
}

impl Server {
    pub fn new(c: ServerPEMOptions) -> Self {
        let config = load::load_ser_config_from_pem(&c, None).expect("tls server config valid");
        Server {
            ta: TlsAcceptor::from(Arc::new(config)),
            option_cache: c.clone(),
            ext_fields: Some(MapExtFields::default()),
        }
    }

    async fn handshake(
        &self,
        _cid: ruci::net::CID,
        mut conn: ruci::net::Conn,
        b: Option<BytesMut>,
        a: Option<ruci::net::Addr>,
    ) -> anyhow::Result<ruci::map::MapResult> {
        if let Some(pre_read_data) = b {
            debug!("tls server got pre_read_data, init with EarlyDataWrapper");
            let nc = EarlyDataWrapper::from(pre_read_data, conn);

            conn = Box::new(nc);
        }

        let c = self.ta.accept(conn).await?;

        // todo add SeverTLSConnDescriber as data
        Ok(MapResult::new_c(Box::new(c)).a(a).build())
    }
}

// pub struct SeverTLSConnDescriber {}

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
                Err(e) => MapResult::from_e(e.context("TLS server handshake failed")),
            }
        } else {
            MapResult::from_err_str("tls only support tcplike stream")
        }
    }
}
