/*!
Defines facilities for tls insecure for rustls 0.21
 */
use std::{fs::File, io::BufReader, sync::Arc, time::SystemTime};

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

pub fn read_certs_from_file(
    cert_path: &str,
    key_path: &str,
) -> anyhow::Result<(Vec<rustls::Certificate>, rustls::PrivateKey)> {
    let mut cert_chain_reader = BufReader::new(File::open(cert_path)?);
    let certs = rustls_pemfile::certs(&mut cert_chain_reader)?
        .into_iter()
        .map(rustls::Certificate)
        .collect();

    let mut key_reader = BufReader::new(File::open(key_path)?);
    // if the file starts with "BEGIN RSA PRIVATE KEY"
    // let mut keys = rustls_pemfile::rsa_private_keys(&mut key_reader)?;
    // if the file starts with "BEGIN PRIVATE KEY"
    let mut keys = rustls_pemfile::pkcs8_private_keys(&mut key_reader)?;

    assert_eq!(keys.len(), 1);
    let key = rustls::PrivateKey(keys.remove(0));

    Ok((certs, key))
}
