use log::{debug, info, log_enabled};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc::{self, Receiver},
};

use crate::Name;

use super::*;

#[derive(Clone, Debug)]
pub struct Direct;
impl Name for Direct {
    fn name(&self) -> &'static str {
        "direct"
    }
}

#[async_trait]
impl Mapper for Direct {
    /// dial params.a.
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        if params.a.is_none() {
            return MapResult::err_str(&format!("cid: {}, direct need params.a, got empty", cid));
        }
        let a = params.a.unwrap();

        if log_enabled!(log::Level::Debug) {
            debug!("direct dial, {} , {}", a, cid);
        }

        let asor = a.get_socket_addr_or_resolve();

        match asor {
            Ok(aso) => {
                let r = TcpStream::connect(aso).await;

                match r {
                    Ok(c) => {
                        return MapResult::c(Box::new(c));
                    }
                    Err(e) => return MapResult::from_err(e),
                }
            }
            Err(e) => return MapResult::from_err(e),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TcpDialer {
    pub addr: Option<net::Addr>,
}

impl Name for TcpDialer {
    fn name(&self) -> &'static str {
        "tcp dialer"
    }
}

impl TcpDialer {
    pub async fn dial_addr(a: &net::Addr) -> MapResult {
        let r = TcpStream::connect(a.get_socket_addr().unwrap()).await;

        match r {
            Ok(c) => {
                return MapResult::c(Box::new(c));
            }
            Err(e) => return MapResult::from_err(e),
        }
    }
}

#[async_trait]
impl Mapper for TcpDialer {
    fn get_target_addr(&self) -> Option<net::Addr> {
        self.addr.clone()
    }

    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::None => match params.d {
                Some(d) => {
                    if let Some(d) = d.calculated_data {
                        match d {
                            AnyData::Addr(a) => return TcpDialer::dial_addr(&a).await,
                            AnyData::A(arc) => {
                                let mut v = arc.lock().await;
                                let a = v.downcast_mut::<net::Addr>();
                                match a {
                                    Some(a) => return TcpDialer::dial_addr(a).await,
                                    None => {
                                        return MapResult::err_str(
                                            "tcp dialer got dial addr paramater but it's None",
                                        )
                                    }
                                }
                            }
                            AnyData::B(mut b) => {
                                let a = b.downcast_mut::<net::Addr>();
                                match a {
                                    Some(a) => return TcpDialer::dial_addr(a).await,
                                    None => {
                                        return MapResult::err_str(
                                            "tcp dialer got dial addr paramater but it's None",
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
                None => {
                    if let Some(a) = &self.addr {
                        return TcpDialer::dial_addr(a).await;
                    }
                    return MapResult::err_str(&format!(
                        "cid: {}, tcp dialer can't dial without a address",
                        cid
                    ));
                }
            },

            Stream::TCP(_) => {
                return MapResult::err_str("tcp dialer can't dial when a tcp conn already exists")
            }
            Stream::UDP(_) => {
                return MapResult::err_str("tcp dialer can't dial when a udp conn already exists")
            }
            _ => {
                return MapResult::err_str(
                    "tcp dialer can't dial when a stream generator already exists",
                )
            }
        }
        unimplemented!()
    }
}

#[derive(Clone, Debug)]
pub struct TcpStreamGenerator {
    pub addr: Option<net::Addr>,
}

impl Name for TcpStreamGenerator {
    fn name(&self) -> &'static str {
        "tcp listener"
    }
}
impl TcpStreamGenerator {
    pub async fn listen_addr(
        a: &net::Addr,
        shutdown_rx: oneshot::Receiver<()>,
    ) -> io::Result<Receiver<net::Stream>> {
        let r = TcpListener::bind(a.clone().get_socket_addr().unwrap()).await;

        match r {
            Ok(listener) => {
                let (tx, rx) = mpsc::channel(100); //todo: change this

                tokio::spawn(async move {
                    tokio::select! {
                        r = async{
                            let   lastr  ;
                            loop {
                                let r = listener.accept().await;


                                if let Err(e) = r {
                                    info!("loop tcp ended,listen e: {}", e);
                                    lastr = Err(e);
                                    break;
                                }

                                let (tcpstream, raddr) = r.unwrap();
                                info!("new accepted tcp, raddr: {}", raddr);

                                let r = tx.send(Stream::TCP(Box::new(tcpstream))).await;
                                if let Err(e) = r {
                                    info!("loop tcp ended,tx e: {}", e);
                                    lastr = Err(io::Error::other(format!("{}",e)));
                                    break;
                                }
                            }

                            lastr

                        } =>{
                            r
                        }

                        _ = shutdown_rx => {
                            info!("terminating accept loop");
                            Ok(())
                        }
                    }
                });
                return Ok(rx);
            }
            Err(e) => Err(e),
        }
    }

    pub async fn listen_addr_forever(a: &net::Addr) -> io::Result<Receiver<net::Stream>> {
        let r = TcpListener::bind(a.clone().get_socket_addr().unwrap()).await;

        match r {
            Ok(listener) => {
                let (tx, rx) = mpsc::channel(100); //todo: change this

                tokio::spawn(async move {
                    loop {
                        let r = listener.accept().await;

                        if let Err(e) = r {
                            info!("loop tcp ended,listen e: {}", e);
                            break;
                        }

                        let (tcpstream, raddr) = r.unwrap();
                        info!("new accepted tcp, raddr: {}", raddr);

                        let r = tx.send(Stream::TCP(Box::new(tcpstream))).await;
                        if let Err(e) = r {
                            info!("loop tcp ended,tx e: {}", e);
                            break;
                        }
                    }
                });
                return Ok(rx);
            }
            Err(e) => Err(e),
        }
    }
}

#[async_trait]
impl Mapper for TcpStreamGenerator {
    async fn maps(&self, _cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let a = match params.a.as_ref() {
            Some(a) => a,
            None => self.addr.as_ref().unwrap(),
        };

        let r = match params.shutdown_rx {
            Some(rx) => TcpStreamGenerator::listen_addr(a, rx).await,
            None => TcpStreamGenerator::listen_addr_forever(a).await,
        };

        match r {
            Ok(rx) => MapResult::gs(rx, CID::default()),
            Err(e) => MapResult::from_err(e),
        }
    }
}
