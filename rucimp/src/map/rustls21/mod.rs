/*!
Defines facilities for tls insecure for rustls 0.21
 */
use std::{sync::Arc, time::SystemTime};

use rustls::{client::ServerCertVerified, Certificate, ClientConfig, ServerName};
use tracing::debug;

#[derive(Debug, Default)]
pub struct ClientOptions {
    pub is_insecure: bool,
    pub alpn: Option<Vec<String>>,
}

pub(crate) fn cc(opt: ClientOptions) -> ClientConfig {
    let root_store = rustls::RootCertStore::empty();
    let mut cc = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    if opt.is_insecure {
        cc.dangerous()
            .set_certificate_verifier(Arc::new(SuperDanVer {}));
    }
    if let Some(a) = opt.alpn {
        cc.alpn_protocols = a.iter().map(|s| s.as_bytes().to_vec()).collect()
    }
    cc
}

#[derive(Debug)]
pub struct SuperDanVer {}

impl rustls::client::ServerCertVerifier for SuperDanVer {
    fn verify_server_cert(
        &self,
        _end_entity: &Certificate,
        _intermediates: &[Certificate],
        _server_name: &ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: SystemTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        debug!("superdanver called");
        //if !server_name.eq(&self.domain) {}//server_name是client自己提供的,
        //因为不验证cert, 所以没有必要和自己比较

        Ok(ServerCertVerified::assertion())
    }
}
