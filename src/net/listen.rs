use anyhow::{bail, Ok};
use tokio::net::{TcpListener, UnixListener};

use crate::net::{self, Stream};

pub enum Listener {
    TCP(TcpListener),

    #[cfg(unix)]
    UNIX(UnixListener),
}

pub async fn listen(a: &net::Addr) -> anyhow::Result<Listener> {
    match a.network {
        net::Network::TCP => {
            let r = TcpListener::bind(a.get_socket_addr().expect("a has socket addr")).await?;
            return Ok(Listener::TCP(r));
        }
        #[cfg(unix)]
        net::Network::Unix => {
            let r = UnixListener::bind(a.get_name().expect("a has a name"))?;
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

                let p = unix_soa
                    .as_pathname()
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                let a = net::Addr {
                    addr: net::NetAddr::Name(p, 0),
                    network: net::Network::Unix,
                };

                return Ok((Stream::TCP(Box::new(unix_stream)), a));
            }
        }
    }
}
