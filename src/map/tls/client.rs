use self::map::CID;

use super::*;

#[derive(Debug, Clone)]
pub struct Client {
    pub domain: String,
    pub is_insecure: bool,
    client_config: Arc<ClientConfig>,
}

impl<IO> crate::Name for tokio_rustls::client::TlsStream<IO> {
    fn name(&self) -> &str {
        "tokio_rustls_client_stream"
    }
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

        if is_insecure {
            config
                .dangerous()
                .set_certificate_verifier(Arc::new(SuperDanVer {}));
        }

        Client {
            domain: domain.to_string(),
            is_insecure,
            client_config: Arc::new(config),
        }
    }
}

struct SuperDanVer {}

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
        _cid: CID,
        conn: net::Conn,
        b: Option<BytesMut>,
        a: Option<net::Addr>,
    ) -> io::Result<MapResult> {
        let connector = TlsConnector::from(self.client_config.clone());

        let mut new_c = connector
            .connect(
                rustls::ServerName::try_from(self.domain.as_str()).unwrap(),
                conn,
            )
            .await?;

        if let Some(ed) = b {
            new_c.write_all(&ed).await?;
            new_c.flush().await?;
        }

        Ok(MapResult {
            a,
            b: None,
            c: map::Stream::TCP(Box::new(new_c)),
            d: Some(map::AnyData::B(Box::new(ClientTLSConnDescriber {}))),
            e: None,
            new_id: None,
        })
    }
}

impl Name for Client {
    fn name(&self) -> &'static str {
        "tls_client"
    }
}
#[async_trait]
impl map::Mapper for Client {
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
                Ok(r) => r,
                Err(e) => MapResult::from_err(e),
            }
        } else {
            MapResult::err_str("tls only support tcplike stream")
        }
    }
}
