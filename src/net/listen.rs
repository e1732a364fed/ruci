use std::{fs::remove_file, path::PathBuf};

use anyhow::{bail, Context, Ok};
use log::{debug, warn};
use tokio::net::TcpListener;

#[cfg(unix)]
use tokio::net::UnixListener;

use crate::net::{self, Stream};

pub enum Listener {
    TCP(TcpListener),

    #[cfg(unix)]
    UNIX(UnixListener),
}

pub async fn listen(a: &net::Addr) -> anyhow::Result<Listener> {
    match a.network {
        net::Network::TCP => {
            let r = TcpListener::bind(a.get_socket_addr().expect("a has socket addr"))
                .await
                .context("tcp listen failed")?;
            return Ok(Listener::TCP(r));
        }
        #[cfg(unix)]
        net::Network::Unix => {
            let filen = a.get_name().expect("a has a name");
            let p = PathBuf::from(filen);

            // is_file returns false for unix domain socket

            if p.exists() && !p.is_dir() {
                warn!(
                    "unix listen: previous {:?} exists, will delete it for new listening.",
                    p
                );
                remove_file(p.clone()).context("unix listen try remove previous file failed")?;
            }
            let r = UnixListener::bind(p).context("unix listen failed")?;
            return Ok(Listener::UNIX(r));
        }
        _ => bail!("listen not implemented for this network: {}", a.network),
    }
}

impl Listener {
    pub fn network(&self) -> net::Network {
        match self {
            Listener::TCP(_) => net::Network::TCP,
            #[cfg(unix)]
            Listener::UNIX(_) => net::Network::Unix,
        }
    }
    pub async fn accept(&self) -> anyhow::Result<(Stream, net::Addr)> {
        match self {
            Listener::TCP(tl) => {
                let (tcp_stream, tcp_soa) = tl.accept().await?;

                let a = net::Addr {
                    addr: net::NetAddr::Socket(tcp_soa),
                    network: net::Network::TCP,
                };
                return Ok((Stream::TCP(Box::new(tcp_stream)), a));
            }
            #[cfg(unix)]
            Listener::UNIX(ul) => {
                let (unix_stream, unix_soa) = ul.accept().await?;

                debug!("unix got {:?}", unix_soa);
                let a = if unix_soa.is_unnamed() {
                    net::Addr {
                        addr: net::NetAddr::Name("".to_string(), 0),
                        network: net::Network::Unix,
                    }
                } else {
                    let p = unix_soa
                        .as_pathname()
                        .unwrap()
                        .to_string_lossy()
                        .to_string();

                    net::Addr {
                        addr: net::NetAddr::Name(p, 0),
                        network: net::Network::Unix,
                    }
                };

                return Ok((Stream::TCP(Box::new(unix_stream)), a));
            }
        }
    }
}
