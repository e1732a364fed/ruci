/*!
Defines facilities for rustls 0.21.

rustls 0.21 和 0.22 有很大不同, 截至 24.3.21, ruci包的 rustls 使用的是
0.22, 但 rucimp 包中的 s2n-quic 和 quinn 包都使用的是 rustls 0.21,
故只能在 rucimp 包再实现一个 rustls 0.21 的接口

used by quinn and quic mod
 */
pub use rustls::ServerConfig;

use std::{
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use ruci::map::tls_config::*;

use anyhow::{bail, Result};
use data_source::DataSource;
use rustls::{client::ServerCertVerified, Certificate, ClientConfig, PrivateKey, ServerName};
use rustls_pemfile::{read_one, Item};
use tracing::debug;

pub fn cc(opt: ClientOptions, data_source: &DataSource) -> Result<ClientConfig> {
    let mut root_store = rustls::RootCertStore::empty();

    root_store.add_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
        rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
            ta.subject,
            ta.spki,
            ta.name_constraints,
        )
    }));

    if let Some(c) = opt.cert {
        let c = load_certs(&c, data_source)?;
        for c in c {
            root_store.add(&c)?;
        }
    }

    let mut cc = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    if opt.insecure {
        cc.dangerous()
            .set_certificate_verifier(Arc::new(SuperDanVer {}));
    }
    if let Some(a) = opt.alpn {
        cc.alpn_protocols = a.iter().map(|s| s.as_bytes().to_vec()).collect()
    }
    Ok(cc)
}

pub fn sc(opt: ServerOptions, data_source: &DataSource) -> Result<ServerConfig> {
    use anyhow::Context;

    let (c, k) = read_certs_from_file(opt.cert.as_str(), opt.key.as_str(), data_source)
        .context("read_certs_from_file failed")?;

    let mut config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(c, k)
        .context("with_single_cert failed")?;

    if let Some(a) = opt.alpn {
        config.alpn_protocols = a.iter().map(|s| s.as_bytes().to_vec()).collect()
    }
    Ok(config)
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

pub fn load_key(path: &Path, data_source: &DataSource) -> Result<PrivateKey> {
    let key = PathBuf::from(path);
    let key_str = data_source.read_to_string(key)?;

    match read_one(&mut BufReader::new(key_str.as_bytes())) {
        Ok(Some(Item::RSAKey(data) | Item::PKCS8Key(data) | Item::ECKey(data))) => {
            Ok(PrivateKey(data))
        }
        Ok(_) => bail!("invalid key in {}, not rsa/pkcs8/ec", path.display()),

        Err(e) => Err(e.into()),
    }
}

/// 注：一个文件有多个 cert 的情况一般是 fullchain
pub fn load_certs(cert: &str, data_source: &DataSource) -> Result<Vec<rustls::Certificate>> {
    let cert = PathBuf::from(cert);
    let cert_str = data_source.read_to_string(cert)?;

    let mut cert_chain_reader = BufReader::new(cert_str.as_bytes());
    let certs = rustls_pemfile::certs(&mut cert_chain_reader)?
        .into_iter()
        .map(rustls::Certificate)
        .collect();
    Ok(certs)
}

pub fn read_certs_from_file(
    cert: &str,
    key: &str,
    data_source: &DataSource,
) -> Result<(Vec<rustls::Certificate>, rustls::PrivateKey)> {
    let certs = load_certs(cert, data_source)?;

    let key = load_key(Path::new(key), data_source)?;

    Ok((certs, key))
}
