/*!
Defines facilities for rustls 0.21

rustls 0.21 和 0.22 有很大不同, 截至 24.3.21, ruci包的 rustls 使用的是
0.22, 但 rucimp 包中的 s2n-quic 和 quinn 包都使用的是 rustls 0.21,
故只能在 rucimp 包再实现一个 rustls 0.21 的接口

used by quinn and quic mod
 */
use std::{fs::File, io::BufReader, path::Path, sync::Arc, time::SystemTime};

use anyhow::bail;
use rustls::{
    client::ServerCertVerified, Certificate, ClientConfig, PrivateKey, ServerConfig, ServerName,
};
use rustls_pemfile::{read_one, Item};
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

#[derive(Debug, Default)]
pub struct ServerOptions {
    pub alpn: Option<Vec<String>>,

    pub cert_path: String,
    pub key_path: String,
}

pub fn sc(opt: ServerOptions) -> anyhow::Result<ServerConfig> {
    let (c, k) = read_certs_from_file(opt.cert_path.as_str(), opt.key_path.as_str())?;

    let mut config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(c, k)?;

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

pub fn load_key(path: &Path) -> anyhow::Result<PrivateKey> {
    match read_one(&mut BufReader::new(File::open(path)?)) {
        Ok(Some(Item::RSAKey(data) | Item::PKCS8Key(data) | Item::ECKey(data))) => {
            Ok(PrivateKey(data))
        }
        Ok(_) => bail!("invalid key in {}, not rsa/pkcs8/ec", path.display()),

        Err(e) => Err(e.into()),
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

    let key = load_key(Path::new(key_path))?;

    Ok((certs, key))
}