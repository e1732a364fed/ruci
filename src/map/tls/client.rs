use macro_map::{map_ext_fields, MapExt};
use rustls::{
    client::danger::ServerCertVerified,
    pki_types::{CertificateDer, ServerName, UnixTime},
    server::WebPkiClientVerifier,
    ClientConfig,
};
use tokio::io::AsyncWriteExt;

use self::{
    map::{MapExt, MapExtFields, CID},
    net::Stream,
};

use super::*;

#[map_ext_fields]
#[derive(Debug, Clone, MapExt)]
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

fn default_cc() -> ClientConfig {
    ClientConfig::builder()
        .with_root_certificates(default_rcs())
        .with_no_client_auth()
}

#[derive(Debug, Default)]
pub struct ClientOptions {
    pub domain: String,
    pub is_insecure: bool,
    pub alpn: Option<Vec<String>>,
}

impl Client {
    pub fn new(opt: ClientOptions) -> Self {
        let mut config = default_cc();

        if opt.is_insecure {
            config
                .dangerous()
                .set_certificate_verifier(Arc::new(SuperDanVer {}));
        }
        if let Some(a) = opt.alpn {
            config.alpn_protocols = a.iter().map(|s| s.as_bytes().to_vec()).collect()
        }

        Client {
            domain: opt.domain,
            is_insecure: opt.is_insecure,
            client_config: Arc::new(config),
            ext_fields: Some(MapExtFields::default()),
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
        //if !server_name.eq(&self.domain) {}//server_name是client自己提供的,
        //因为不验证cert, 所以没有必要和自己比较

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
        let root_certs = default_rcs();

        WebPkiClientVerifier::builder(Arc::new(root_certs))
            .build()
            .expect("WebPkiClientVerifier::builder build ok ")
            .supported_verify_schemes()
    }
}

// pub struct ClientTLSConnDescriber {}

impl Client {
    async fn handshake(
        &self,
        conn: net::Conn,
        b: Option<BytesMut>,
        a: Option<net::Addr>,
    ) -> anyhow::Result<MapResult> {
        let connector = TlsConnector::from(self.client_config.clone());

        let new_c = connector
            .connect(
                ServerName::try_from(self.domain.clone()).expect("domain string to serverName ok"),
                conn,
            )
            .await?;

        let mrb = MapResult::builder().a(a);
        // todo: add ClientTLSConnDescriber as data

        let mut bc = Box::new(new_c);

        if self.is_tail_of_chain() {
            if let Some(ed) = b {
                //debug!("tls client writing ed, because is_tail_of_chain");
                bc.write_all(&ed).await?;
                bc.flush().await?;
            }
            Ok(mrb.c(Stream::c(bc)).build())
        } else {
            Ok(mrb.b(b).c(Stream::c(bc)).build())
        }
    }
}

impl Name for Client {
    fn name(&self) -> &'static str {
        "tls_client"
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
        if let map::Stream::Conn(conn) = conn {
            let r = self.handshake(conn, params.b, params.a).await;
            match r {
                Ok(r) => r,
                Err(e) => MapResult::from_e(e.context("TLS client handshake failed")),
            }
        } else {
            MapResult::err_str(&format!(
                "tls client only support tcplike stream, got {}",
                &conn
            ))
        }
    }
}
