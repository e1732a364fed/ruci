use tokio::net::TcpStream;

use crate::Name;

use super::*;

pub struct TcpDialer {
    addr: Option<net::Addr>,
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

impl Name for TcpDialer {
    fn name(&self) -> &'static str {
        "[tcp dialer]"
    }
}

#[async_trait]
impl Mapper for TcpDialer {
    async fn maps(
        &self,
        cid: u32, //state 的 id
        _behavior: ProxyBehavior,
        params: MapParams,
    ) -> MapResult {
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
        }
        unimplemented!()
    }
}
