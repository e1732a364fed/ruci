use log::{debug, info, log_enabled};
use macro_mapper::{common_mapper_field, CommonMapperExt, DefaultMapperExt};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::mpsc::{self, Receiver},
};

use super::*;
use crate::Name;
use crate::{map, net::Addr};

#[derive(DefaultMapperExt, Debug, Default, Clone)]
pub struct BlackHole {}

impl Name for BlackHole {
    fn name(&self) -> &str {
        "blackhole"
    }
}

#[async_trait]
impl Mapper for BlackHole {
    /// always consume the stream, ignore all params.
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        if params.c.is_some() {
            info!("{cid} consumed by blackhole");
        }
        return MapResult::default();
    }
}

/// only use CommonMapperExt's is_tail_of_chain. won't use configured_target_addr;
/// if you want to set configured_target_addr, maybe you should use TcpDialer
#[common_mapper_field]
#[derive(Clone, Debug, Default, CommonMapperExt)]
pub struct Direct {}
impl Name for Direct {
    fn name(&self) -> &'static str {
        "direct"
    }
}

#[async_trait]
impl Mapper for Direct {
    /// dial params.a.
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let a = match params.a {
            Some(a) => a,
            None => {
                return MapResult::err_str(&format!("{}, direct need params.a, got empty", cid))
            }
        };

        if log_enabled!(log::Level::Debug) {
            debug!("direct dial, {} , {}", a, cid);
        }

        //todo: DNS 功能

        let asor = a.get_socket_addr_or_resolve();

        match asor {
            Ok(aso) => {
                let r = TcpStream::connect(aso).await;

                match r {
                    Ok(mut c) => {
                        if self.is_tail_of_chain() && params.b.is_some() {
                            let rw = c
                                .write_all(params.b.as_ref().expect("param.b is some"))
                                .await;
                            if let Err(re) = rw {
                                return MapResult::from_io_err(re);
                            }
                            return MapResult::c(Box::new(c));
                        }
                        return MapResult::newc(Box::new(c)).b(params.b).build();
                    }
                    Err(e) => return MapResult::from_io_err(e),
                }
            }
            Err(e) => return MapResult::from_e(e),
        }
    }
}

#[common_mapper_field]
#[derive(Clone, Debug, Default, CommonMapperExt)]
pub struct TcpDialer {}

impl Name for TcpDialer {
    fn name(&self) -> &'static str {
        "tcp_dialer"
    }
}

impl TcpDialer {
    ///  panic if dial_a is invalid. todo: try not panic
    pub async fn dial_addr(
        dial_a: &net::Addr,
        pass_a: Option<net::Addr>,
        pass_b: Option<BytesMut>,
    ) -> MapResult {
        //todo: DNS 功能

        let r = TcpStream::connect(dial_a.get_socket_addr().expect("dial_a has socket addr")).await;

        match r {
            Ok(c) => MapResult::newc(Box::new(c)).a(pass_a).b(pass_b).build(),
            Err(e) => MapResult::from_io_err(e),
        }
    }
}

#[async_trait]
impl Mapper for TcpDialer {
    /// try the paramater first, if no addr was given, use configured addr.
    /// 注意, dial addr 和target addr (params.a) 不一样
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::None => match params.d {
                Some(d) => {
                    if let Some(d) = d.calculated_data {
                        match d {
                            AnyData::Addr(a) => {
                                return TcpDialer::dial_addr(&a, params.a, params.b).await
                            }
                            AnyData::A(arc) => {
                                let a: Option<Addr>;
                                {
                                    let v = arc.lock();
                                    let aa = v.downcast_ref::<net::Addr>();
                                    a = aa.map(|x| x.clone());
                                }
                                match a {
                                    Some(a) => {
                                        return TcpDialer::dial_addr(&a, params.a, params.b).await;
                                    }
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
                                    Some(a) => {
                                        return TcpDialer::dial_addr(a, params.a, params.b).await
                                    }
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
                    if let Some(configured_dial_a) = &self.fixed_target_addr {
                        return TcpDialer::dial_addr(configured_dial_a, params.a, params.b).await;
                    }
                    return MapResult::err_str(&format!(
                        "{}, tcp dialer can't dial without an address",
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

/// 不命名为TcpListener 只是因为不希望有重名
#[common_mapper_field]
#[derive(CommonMapperExt, Clone, Debug, Default)]
pub struct TcpStreamGenerator {}

impl Name for TcpStreamGenerator {
    fn name(&self) -> &'static str {
        "tcp_listener"
    }
}
impl TcpStreamGenerator {
    pub async fn listen_addr(
        a: &net::Addr,
        shutdown_rx: oneshot::Receiver<()>,
    ) -> io::Result<Receiver<MapResult>> {
        let r = TcpListener::bind(a.clone().get_socket_addr().expect("a has socket addr")).await;

        match r {
            Ok(listener) => {
                let (tx, rx) = mpsc::channel(100); //todo: change this

                tokio::spawn(async move {
                    tokio::select! {
                        r = async{
                            let   lastr  ;
                            loop {
                                let r = listener.accept().await;

                                let (tcpstream, raddr) =match r{
                                    Ok(x) => x,
                                    Err(e) => {
                                        let e = anyhow!("listen tcp ended by listen e: {}",e);
                                        info!("{}", e);
                                        lastr = Err(e);
                                        break;
                                    },
                                };

                                debug!("new accepted tcp, raddr: {}", raddr);

                                let pa = Addr{ addr:net::NetAddr::Socket(raddr), network: net::Network::TCP };

                                let r = tx.send(
                                    MapResult::newc(Box::new(tcpstream))
                                    .d(AnyData::Addr(pa))
                                    .build(),

                                ).await;
                                if let Err(e) = r {
                                    let e = anyhow!("listen tcp ended by tx e: {}",e);
                                    info!("{}", e);
                                    lastr = Err(e);
                                    break;
                                }
                            }

                            lastr

                        } =>{
                            r
                        }

                        _ = shutdown_rx => {
                            info!("terminating tcp listen");
                            Ok(())
                        }
                    }
                });
                Ok(rx)
            }
            Err(e) => Err(e),
        }
    }

    /// not recommended
    pub async fn listen_addr_forever(a: &net::Addr) -> io::Result<Receiver<MapResult>> {
        let r = TcpListener::bind(a.clone().get_socket_addr().expect("a has socket addr")).await;

        match r {
            Ok(listener) => {
                let (tx, rx) = mpsc::channel(100); //todo: change this

                tokio::spawn(async move {
                    loop {
                        let r = listener.accept().await;

                        let (tcpstream, raddr) = match r {
                            Ok(x) => x,
                            Err(e) => {
                                info!("loop tcp ended,listen e: {}", e);
                                break;
                            }
                        };

                        info!("new accepted tcp, raddr: {}", raddr);

                        let pa = Addr {
                            addr: net::NetAddr::Socket(raddr),
                            network: net::Network::TCP,
                        };

                        let r = tx
                            .send(
                                MapResult::newc(Box::new(tcpstream))
                                    .d(AnyData::Addr(pa))
                                    .build(),
                            )
                            .await;

                        if let Err(e) = r {
                            info!("loop tcp ended,tx e: {}", e);
                            break;
                        }
                    }
                });
                Ok(rx)
            }
            Err(e) => Err(e),
        }
    }
}

#[async_trait]
impl Mapper for TcpStreamGenerator {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let a = match params.a.as_ref() {
            Some(a) => a,
            None => self
                .fixed_target_addr
                .as_ref()
                .expect("TcpStreamGenerator always has a fixed_target_addr"),
        };

        if log_enabled!(log::Level::Debug) {
            debug!("{}, start listen tcp {}", cid, a)
        }

        let r = match params.shutdown_rx {
            Some(rx) => TcpStreamGenerator::listen_addr(a, rx).await,
            None => TcpStreamGenerator::listen_addr_forever(a).await,
        };

        match r {
            Ok(rx) => MapResult::newgs(rx).build(),
            Err(e) => MapResult::builder().e(e.into()).build(), //MapResult::from_err(e),
        }
    }
}
