use macro_map::*;

use crate::map::{MapBox, ToMapBox};

use self::map::{MapExtFields, CID};

use super::*;

#[derive(Debug, Clone, Default)]
pub struct ServerOptions {
    pub addr: String,
    pub cert: PathBuf,
    pub key: PathBuf,
    pub alpn: Option<Vec<String>>,
}

impl ToMapBox for ServerOptions {
    fn to_map_box(&self) -> MapBox {
        let a = Server::new(self.clone());
        Box::new(a)
    }
}

// todo: 添加 alpn 和 tls_min_v
#[map_ext_fields]
#[derive(Clone, MapExt)]
pub struct Server {
    pub option_cache: ServerOptions,
    ta: TlsAcceptor,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ruci::tls::Server, {:?}", self.option_cache)
    }
}
impl<IO> crate::Name for tokio_rustls::server::TlsStream<IO> {
    fn name(&self) -> &str {
        "tokio_rustls_server_stream"
    }
}

impl Server {
    pub fn new(c: ServerOptions) -> Self {
        let config = load::load_ser_config(&c).expect("tls server config valid");
        Server {
            ta: TlsAcceptor::from(Arc::new(config)),
            option_cache: c.clone(),
            ext_fields: Some(MapExtFields::default()),
        }
    }

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
        Ok(MapResult::new_c(Box::new(c)).a(a).build())
    }
}

// pub struct SeverTLSConnDescriber {}

impl Name for Server {
    fn name(&self) -> &'static str {
        "tls_server"
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
        if let crate::net::Stream::Conn(conn) = conn {
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
