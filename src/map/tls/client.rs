use rustls::{
    client::danger::ServerCertVerified,
    pki_types::{CertificateDer, Der, ServerName, TrustAnchor, UnixTime},
    server::WebPkiClientVerifier,
    ClientConfig,
};

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
        root_certs.extend(
            webpki_roots::TLS_SERVER_ROOTS
                .0
                .iter()
                .map(|ta| TrustAnchor {
                    subject: ta.subject.into(),
                    subject_public_key_info: ta.spki.into(),
                    name_constraints: ta.name_constraints.map(|u| Der::from(u)),
                }),
        );
        let mut config = ClientConfig::builder()
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

#[derive(Debug)]
struct SuperDanVer {}

impl rustls::client::danger::ServerCertVerifier for SuperDanVer {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        debug!("superdanver called");
        //if !server_name.eq(&self.domain) {}//server_name是client自己提供的，
        //在不验证cert的情况下，没有必要和自己比较

        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        let mut root_certs = rustls::RootCertStore::empty();
        root_certs.extend(
            webpki_roots::TLS_SERVER_ROOTS
                .0
                .iter()
                .map(|ta| TrustAnchor {
                    subject: ta.subject.into(),
                    subject_public_key_info: ta.spki.into(),
                    name_constraints: ta.name_constraints.map(|u| Der::from_slice(u)),
                }),
        );

        let x = WebPkiClientVerifier::builder(Arc::new(root_certs))
            .build()
            .unwrap()
            .supported_verify_schemes();

        x
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
            .connect(ServerName::try_from(self.domain.clone()).unwrap(), conn)
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
