//https://github.com/async-rs/async-tls/blob/master/examples/server/src/main.rs

use rustls::{
    pki_types::{
        CertificateDer, PrivateKeyDer, PrivatePkcs1KeyDer, PrivatePkcs8KeyDer, PrivateSec1KeyDer,
    },
    server::NoClientAuth,
    ServerConfig,
};
use std::{fs::File, path::PathBuf, sync::Arc};

use rustls_pemfile::{certs, read_one, Item};
use std::io::{self, BufReader};

use super::server::ServerOptions;

pub fn load_ser_config(options: &ServerOptions) -> io::Result<ServerConfig> {
    let c = options.cert.clone();
    let certs = load_certs(&c)?;
    debug_assert!(!certs.is_empty());
    let k = options.key.clone();
    let key = load_keys(&k)?;

    //todo: we don't use client authentication yet
    let config = rustls::ServerConfig::builder()
        .with_client_cert_verifier(Arc::new(NoClientAuth))
        .with_single_cert(certs, key)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

    Ok(config)
}

/// Load the passed certificates file
fn load_certs(path: &PathBuf) -> io::Result<Vec<CertificateDer<'static>>> {
    Ok(certs(&mut BufReader::new(File::open(path)?))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("{:?}", e)))?
        .into_iter()
        .map(CertificateDer::from)
        .collect())
}

fn load_keys(path: &PathBuf) -> io::Result<PrivateKeyDer<'static>> {
    match read_one(&mut BufReader::new(File::open(path)?)) {
        Ok(Some(Item::PKCS8Key(data))) => Ok(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(data))),
        Ok(Some(Item::RSAKey(data))) => Ok(PrivateKeyDer::Pkcs1(PrivatePkcs1KeyDer::from(data))),
        Ok(Some(Item::ECKey(data))) => Ok(PrivateKeyDer::Sec1(PrivateSec1KeyDer::from(data))),
        Ok(x) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid key in {:?}, {:?}", &path, x),
        )),
        Err(e) => Err(io::Error::new(io::ErrorKind::InvalidInput, e)),
    }
}

#[cfg(test)]
mod test {
    use std::{env, path::PathBuf};

    use super::*;

    #[test]
    fn test_load_key() {
        //println!("{:?}", env::current_dir()); //ruci
        std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/resource")).unwrap();

        let mut path = PathBuf::new();
        path.push("test.key");

        let r = load_keys(&path);
        match r {
            Ok(pk) => {
                println!("{:?}", pk);
            }
            Err(e) => panic!("failed, {}", e),
        }
    }

    #[test]
    fn test_load_cert() {
        std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/resource")).unwrap();

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

    #[test]
    fn test_load_ser_config() {
        let mut path = PathBuf::new();
        path.push("test.crt");

        let mut path2 = PathBuf::new();
        path2.push("test.key");

        let r = load_ser_config(&ServerOptions {
            addr: "todo!()".to_string(),
            cert: path,
            key: path2,
        });

        println!("{:#?}", r);
    }
}

// see https://github.com/async-rs/async-tls/blob/master/examples/client/src/main.rs
// 但是我发现对新版的 rustls_pemfile 来说，内部要 map 一下再 unwrap
// pub async fn client_connector_for_ca_file(cafile: &Path) -> io::Result<TlsConnector> {
//     let mut root_store = rustls::RootCertStore::empty();

//     let ca_bytes = async_std::fs::read(cafile).await?;

//     let cert: Vec<_> = certs(&mut BufReader::new(Cursor::new(ca_bytes)))
//         .map(|x| x.unwrap())
//         .collect();

//     debug_assert_eq!((1, 0), root_store.add_parsable_certificates(&cert));

//     let config = ClientConfig::builder()
//         .with_safe_defaults()
//         .with_root_certificates(root_store)
//         .with_no_client_auth();

//     Ok(TlsConnector::from(Arc::new(config)))
// }
