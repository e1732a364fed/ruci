use std::{fs::remove_file, path::PathBuf};

use anyhow::{bail, Context};
use tokio::net::TcpListener;
use tracing::{info, warn};

#[cfg(unix)]
use tokio::net::UnixListener;

use crate::net::{self, Stream};

use super::{udp_fixed_listen::FixedTargetAddrUDPListener, Addr};

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
            let r = TcpListener::bind(
                laddr
                    .get_socket_addr()
                    .context("listen tcp but has no socket addr")?,
            )
            .await
            .context("tcp listen failed")?;
            Ok(Listener::TCP(r))
        }
        net::Network::UDP => {
            let ft = match opt_fixed_target_addr {
                Some(ft) => ft,
                None => bail!("listen udp requires a fixed_target_addr"),
            };
            Ok(Listener::UDP(
                FixedTargetAddrUDPListener::new(laddr.clone(), ft).await?,
            ))
        }

        #[cfg(unix)]
        net::Network::Unix => {
            let file_n = laddr.get_name().context("listen unix but has no name")?;
            let p = PathBuf::from(file_n.clone());

            remove_unix(&p, true)?;
            let r = UnixListener::bind(p).context("listen unix failed")?;

            Ok(Listener::UNIX((r, file_n)))
        }
        _ => bail!("listen not implemented for this network: {}", laddr.network),
    }
}

fn remove_unix(p: &PathBuf, warn: bool) -> anyhow::Result<()> {
    // is_file returns false for unix domain socket

    if p.exists() && !p.is_dir() && !p.is_file() {
        if warn {
            warn!("unix: previous {:?} exists, will remove it!", p);
        } else {
            info!("removing unix: {:?}", p);
        }
        remove_file(p.clone()).context("unix try remove previous file failed")?;
    }
    Ok(())
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
            Listener::UDP(u) => format!("{}", u.laddr()),

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
                let r = remove_unix(&p, false);
                if let Err(e) = r {
                    warn!("{}", e)
                }
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
