use std::{fs::remove_file, path::PathBuf};

use anyhow::{bail, Context};
use tokio::net::TcpListener;
use tracing::warn;

#[cfg(unix)]
use tokio::net::UnixListener;

use crate::net::{self, Stream};

use super::{udp_listen::FixedTargetAddrUDPListener, Addr};

#[derive(Debug)]
pub enum Listener {
    TCP(TcpListener),

    #[cfg(unix)]
    UNIX((UnixListener, String)),

    UDP(FixedTargetAddrUDPListener),
}

pub async fn listen(
    laddr: &net::Addr,
    opt_fixed_target_addr: Option<net::Addr>,
) -> anyhow::Result<Listener> {
    match laddr.network {
        net::Network::TCP => {
            let r = TcpListener::bind(laddr.get_socket_addr().expect("a has socket addr"))
                .await
                .context("tcp listen failed")?;
            Ok(Listener::TCP(r))
        }
        net::Network::UDP => Ok(Listener::UDP(
            FixedTargetAddrUDPListener::new(laddr.clone(), opt_fixed_target_addr.unwrap()).await?,
        )),

        #[cfg(unix)]
        net::Network::Unix => {
            let file_n = laddr.get_name().expect("a has a name");
            let p = PathBuf::from(file_n.clone());

            // is_file returns false for unix domain socket

            if p.exists() && !p.is_dir() {
                warn!(
                    "unix listen: previous {:?} exists, will delete it for new listening.",
                    p
                );
                remove_file(p.clone()).context("unix listen try remove previous file failed")?;
            }
            let r = UnixListener::bind(p).context("unix listen failed")?;

            Ok(Listener::UNIX((r, file_n)))
        }
        _ => bail!("listen not implemented for this network: {}", laddr.network),
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.clean_up()
    }
}

impl Listener {
    pub fn network(&self) -> net::Network {
        match self {
            Listener::TCP(_) => net::Network::TCP,
            Listener::UDP(_) => net::Network::UDP,

            #[cfg(unix)]
            Listener::UNIX(_) => net::Network::Unix,
        }
    }

    pub fn laddr(&self) -> String {
        match self {
            Listener::TCP(t) => {
                let r = t.local_addr();
                match r {
                    Ok(a) => format!("{a}"),
                    Err(e) => format!("no laddr, e:{e}"),
                }
            }
            Listener::UDP(u) => format!("{}", u.laddr),

            #[cfg(unix)]
            Listener::UNIX(u) => {
                let r = u.0.local_addr();
                match r {
                    Ok(a) => format!("{:?}", a),
                    Err(e) => format!("no laddr, e:{e}"),
                }
            }
        }
    }

    pub fn clean_up(&mut self) {
        match self {
            #[cfg(unix)]
            Listener::UNIX((_, file_n)) => {
                let p = PathBuf::from(file_n.clone());
                if p.exists() && !p.is_dir() {
                    warn!("unix clean up:  will delete {:?}", p);
                    let r = remove_file(p.clone()).context("unix clean up delete file failed");
                    if let Err(e) = r {
                        warn!("{}", e)
                    }
                }
            }
            Listener::UDP(ul) => {
                ul.shutdown();
            }
            _ => {}
        }
    }

    /// returns stream, raddr, laddr
    pub async fn accept(&mut self) -> anyhow::Result<(Stream, net::Addr, net::Addr)> {
        match self {
            Listener::TCP(tl) => {
                let (tcp_stream, tcp_soa) = tl.accept().await?;

                let ra = net::Addr {
                    addr: net::NetAddr::Socket(tcp_soa),
                    network: net::Network::TCP,
                };

                let la = net::Addr {
                    addr: net::NetAddr::Socket(tcp_stream.local_addr()?),
                    network: net::Network::TCP,
                };
                Ok((Stream::Conn(Box::new(tcp_stream)), ra, la))
            }
            #[cfg(unix)]
            Listener::UNIX((ul, _)) => {
                let (unix_stream, unix_soa) = ul.accept().await?;

                //debug!("unix got {:?}", unix_soa); //listen unix will get unnamed
                let ra = Addr::from_unix(unix_soa);
                let la = Addr::from_unix(unix_stream.local_addr()?);

                Ok((Stream::Conn(Box::new(unix_stream)), ra, la))
            }
            Listener::UDP(ul) => {
                let (ac, ra, la) = ul.accept().await?;
                Ok((Stream::AddrConn(ac), ra, la))
            }
        }
    }
}
