use anyhow::Context;
use rcgen::KeyPair;
use rustls::{
    pki_types::{
        CertificateDer, PrivateKeyDer, PrivatePkcs1KeyDer, PrivatePkcs8KeyDer, PrivateSec1KeyDer,
    },
    server::NoClientAuth,
    ServerConfig,
};
use std::{path::PathBuf, sync::Arc};
use tracing::debug;

use rustls_pemfile::{certs, read_one, Item};
use std::io::{self, BufReader};

use super::server::ServerPEMOptions;

/// if `opt_authority` is given, we will use the given cert as CA and generate a new cert for the
/// authority.
pub fn load_ser_config_from_pem(
    options: &ServerPEMOptions,
    opt_authority: Option<&http::uri::Authority>,
) -> anyhow::Result<ServerConfig> {
    let c_pem = options.cert.clone();

    let key_pem = options.key.clone();

    let (certs_der, key_ders) = match opt_authority {
        Some(authority) => {
            // 参考 rcgen/example/sign-leaf-with-ca.rs

            let cacert_pem = c_pem;
            let cakey_pem = key_pem;

            // 重建CA
            let ca_key_pair = KeyPair::from_pem(&cakey_pem)?;
            let ca_params = rcgen::CertificateParams::from_ca_cert_pem(&cacert_pem)?;
            let ca = ca_params.self_signed(&ca_key_pair)?;

            use rand::rng;
            use rand::Rng;

            const NOT_BEFORE_OFFSET: i64 = 60;
            const TTL_SECS: i64 = 31536000;

            let mut params = rcgen::CertificateParams::default();
            params.serial_number = Some(rng().random::<u64>().into());

            let not_before =
                time::OffsetDateTime::now_utc() - time::Duration::seconds(NOT_BEFORE_OFFSET);
            params.not_before = not_before;
            params.not_after = not_before + time::Duration::seconds(TTL_SECS);

            let mut distinguished_name = rcgen::DistinguishedName::new();
            distinguished_name.push(rcgen::DnType::CommonName, authority.host());
            params.distinguished_name = distinguished_name;

            params.subject_alt_names.push(rcgen::SanType::DnsName(
                rcgen::Ia5String::try_from(authority.host()).expect("Failed to create Ia5String"),
            ));

            let key_pair = KeyPair::generate_for(ca_key_pair.algorithm())
                .context("KeyPair::generate() failed")?;

            let cert = params.signed_by(&key_pair, &ca, &ca_key_pair)?;

            let key_der = key_pair.serialize_der();
            let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der));

            (vec![cert.der().clone()], key_der)
        }
        None => {
            let certs_der = load_certs_from_pem(c_pem.as_bytes())?;

            let key_der = load_key_from_pem(key_pem.as_bytes()).context("1")?;

            (certs_der, key_der)
        }
    };

    let mut config = rustls::ServerConfig::builder()
        .with_client_cert_verifier(Arc::new(NoClientAuth))
        .with_single_cert(certs_der, key_ders)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

    if let Some(a) = &options.alpn {
        config.alpn_protocols = a.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    Ok(config)
}

/// Load the passed certificates file
pub fn load_certs(path: &PathBuf) -> io::Result<Vec<CertificateDer<'static>>> {
    let certs_data = std::fs::read(path)?;
    load_certs_from_pem(certs_data.as_slice())
}

pub fn load_certs_from_pem(data: &[u8]) -> io::Result<Vec<CertificateDer<'static>>> {
    Ok(certs(&mut BufReader::new(data))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("{:?}", e)))?
        .into_iter()
        .map(CertificateDer::from)
        .collect())
}

pub fn load_key_from_pem(data: &[u8]) -> io::Result<PrivateKeyDer<'static>> {
    match read_one(&mut BufReader::new(data)) {
        Ok(Some(Item::PKCS8Key(data))) => {
            debug!("key type PKCS8Key");
            Ok(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(data)))
        }
        Ok(Some(Item::RSAKey(data))) => {
            debug!("key type RSAKey");
            Ok(PrivateKeyDer::Pkcs1(PrivatePkcs1KeyDer::from(data)))
        }
        Ok(Some(Item::ECKey(data))) => {
            debug!("key type ECKey");
            Ok(PrivateKeyDer::Sec1(PrivateSec1KeyDer::from(data)))
        }
        Ok(other_item) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "invalid key in {}, {:?}",
                String::from_utf8_lossy(data),
                other_item
            ),
        )),
        Err(e) => Err(io::Error::new(io::ErrorKind::InvalidInput, e)),
    }
}

pub fn load_key(path: &PathBuf) -> io::Result<PrivateKeyDer<'static>> {
    let key_data = std::fs::read(path)?;
    load_key_from_pem(key_data.as_slice())
}

#[cfg(test)]
mod test {
    use std::{env, path::PathBuf};

    use super::*;

    #[test]
    fn test_load_key() {
        let d = concat!(env!("CARGO_MANIFEST_DIR"), "/../../dev_res");
        std::env::set_current_dir(d).unwrap_or_else(|_| panic!("go to {}", d));

        println!("cwd: {:?}", std::env::current_dir().unwrap());

        let mut path = PathBuf::new();
        path.push("test.key");

        let r = load_key(&path);
        match r {
            Ok(pk) => {
                println!("{:?}", pk);
            }
            Err(e) => panic!("failed, {}", e),
        }
    }

    #[test]
    fn test_load_cert() {
        let d = concat!(env!("CARGO_MANIFEST_DIR"), "/../../dev_res");
        std::env::set_current_dir(d).unwrap_or_else(|_| panic!("go to {}", d));

        println!("cwd: {:?}", std::env::current_dir().unwrap());

        let mut path = PathBuf::new();
        path.push("test.crt");

        let r = load_certs(&path);
        match r {
            Ok(pk) => {
                println!("{:?}", pk);
            }
            Err(e) => panic!("failed, {}", e),
        }
    }
}
