use macro_mapper::NoMapperExt;

use crate::map::{MapperBox, ToMapper};

use self::map::CID;

use super::*;

#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub addr: String,
    pub cert: PathBuf,
    pub key: PathBuf,
}

impl ToMapper for ServerOptions {
    fn to_mapper(&self) -> MapperBox {
        let a = Server::new(self.clone());
        Box::new(a)
    }
}

// todo: 添加 alpn 和 tls_minv
#[derive(Clone, NoMapperExt)]
pub struct Server {
    pub option_cache: ServerOptions,
    ta: TlsAcceptor,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ruci::tls::ServerAdder, {:?}", self.option_cache)
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

        Ok(MapResult::newc(Box::new(c))
            .a(a)
            .d(map::AnyData::B(Box::new(SeverTLSConnDescriber {})))
            .build())
    }
}

pub struct SeverTLSConnDescriber {}

impl Name for Server {
    fn name(&self) -> &'static str {
        "tls_server"
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
        if let map::Stream::TCP(conn) = conn {
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
