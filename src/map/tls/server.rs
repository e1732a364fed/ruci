use super::*;

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
impl<IO> crate::Name for tokio_rustls::server::TlsStream<IO> {
    fn name(&self) -> &str {
        "tokio_rustls server stream"
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
            c: Some(map::Stream::TCP(Box::new(c))),
            d: Some(map::AnyData::B(Box::new(SeverTLSConnDescriber {}))),
            e: None,
        })
    }
}

pub struct SeverTLSConnDescriber {}

impl Name for Server {
    fn name(&self) -> &'static str {
        "tls"
    }
}
#[async_trait]
impl map::Mapper for Server {
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
