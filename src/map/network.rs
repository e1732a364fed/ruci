use log::info;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc::{self, Receiver},
};

use crate::Name;

use self::net::TransmissionInfo;

use super::*;

#[derive(Clone)]
pub struct TcpDialer {
    addr: Option<net::Addr>,
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

#[derive(Clone)]
pub struct TcpStreamGenerator {
    addr: Option<net::Addr>,
    oti: Option<Arc<TransmissionInfo>>,
}

impl Name for TcpStreamGenerator {
    fn name(&self) -> &'static str {
        "tcp listener"
    }
}
impl TcpStreamGenerator {
    pub async fn listen_addr(a: &net::Addr) -> io::Result<Receiver<net::Stream>> {
        let r = TcpListener::bind(a.clone().get_socket_addr().unwrap()).await;

        match r {
            Ok(listener) => {
                let (tx, rx) = mpsc::channel(100); //todo: change this

                tokio::spawn(async move {
                    loop {
                        let r = listener.accept().await;

                        if r.is_err() {
                            break;
                        }
                        let (tcpstream, raddr) = r.unwrap();
                        info!("new accepted tcp, raddr: {}", raddr);

                        let r = tx.send(Stream::TCP(Box::new(tcpstream))).await;
                        if r.is_err() {
                            info!("loop tcp ended: {}", r.unwrap_err());

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

        let r = TcpStreamGenerator::listen_addr(a).await;
        match r {
            Ok(rx) => match self.oti.as_ref() {
                Some(ti) => MapResult::gs(rx, CID::new_ordered(&ti.last_connection_id)),
                None => MapResult::gs(rx, CID::new()),
            },
            Err(e) => MapResult::from_err(e),
        };

        unimplemented!()
    }
}
