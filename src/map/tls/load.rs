//https://github.com/async-rs/async-tls/blob/master/examples/server/src/main.rs

use rustls::{Certificate, PrivateKey, ServerConfig};
use std::fs::File;

use rustls_pemfile::{certs, read_one, Item};
use std::io::{self, BufReader};
use std::path::Path;

use super::server::ServerOptions;

pub fn load_ser_config(options: &ServerOptions) -> io::Result<ServerConfig> {
    let certs = load_certs(&options.cert)?;
    debug_assert!(certs.len() > 0);
    let key = load_keys(&options.key)?;

    //todo: we don't use client authentication yet
    let config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

    Ok(config)
}

/// Load the passed certificates file
fn load_certs(path: &Path) -> io::Result<Vec<Certificate>> {
    Ok(certs(&mut BufReader::new(File::open(path)?))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("{:?}", e)))?
        .into_iter()
        .map(Certificate)
        .collect())
}

fn load_keys(path: &Path) -> io::Result<PrivateKey> {
    match read_one(&mut BufReader::new(File::open(path)?)) {
        Ok(Some(Item::RSAKey(data) | Item::PKCS8Key(data) | Item::ECKey(data))) => {
            Ok(PrivateKey(data))
        }
        Ok(x) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid key in {}, {:?}", path.display(), x),
        )),
        Err(e) => Err(io::Error::new(io::ErrorKind::InvalidInput, e)),
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_load_key() {
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
