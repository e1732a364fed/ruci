use log::{debug, info, log_enabled};
use macro_mapper::{common_mapper_field, CommonMapperExt, DefaultMapperExt};
use tokio::{
    net::TcpListener,
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

        let dial_r = a.try_dial().await;
        match dial_r {
            Ok(mut stream) => {
                if matches!(stream, Stream::TCP(_)) && self.is_tail_of_chain() && params.b.is_some()
                {
                    let rw = stream
                        .write_all(params.b.as_ref().expect("param.b is some"))
                        .await;
                    if let Err(re) = rw {
                        let mut e: anyhow::Error = re.into();
                        e = e.context("Direct try write early data");
                        return MapResult::from_e(e);
                    }
                    return MapResult::builder().c(stream).build();
                }
                return MapResult::builder().c(stream).b(params.b).build();
            }
            Err(e) => return MapResult::from_e(e),
        }
    }
}

#[common_mapper_field]
#[derive(Clone, Debug, Default, CommonMapperExt)]
pub struct Dialer {}

impl Name for Dialer {
    fn name(&self) -> &'static str {
        "dialer"
    }
}

impl Dialer {
    pub async fn dial_addr(
        dial_a: &net::Addr,
        pass_a: Option<net::Addr>,
        pass_b: Option<BytesMut>,
    ) -> MapResult {
        let r = dial_a.try_dial().await;

        match r {
            Ok(c) => MapResult::builder().c(c).a(pass_a).b(pass_b).build(),
            Err(e) => MapResult::from_e(e),
        }
    }
}

#[async_trait]
impl Mapper for Dialer {
    /// try the paramater first, if no addr was given, use configured addr.
    /// 注意, dial addr 和target addr (params.a) 不一样
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::None => match params.d {
                Some(d) => {
                    if let Some(d) = d.calculated_data {
                        match d {
                            AnyData::Addr(a) => {
                                return Dialer::dial_addr(&a, params.a, params.b).await
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
                                        return Dialer::dial_addr(&a, params.a, params.b).await;
                                    }
                                    None => {
                                        return MapResult::err_str(
                                            "dialer got dial addr paramater but it's None",
                                        )
                                    }
                                }
                            }
                            AnyData::B(mut b) => {
                                let a = b.downcast_mut::<net::Addr>();
                                match a {
                                    Some(a) => {
                                        return Dialer::dial_addr(a, params.a, params.b).await
                                    }
                                    None => {
                                        return MapResult::err_str(
                                            "dialer got dial addr paramater but it's None",
                                        )
                                    }
                                }
                            }

                            _ => {
                                return MapResult::err_str(&format!(
                                    "{cid} dialer can't dial without an address-",
                                ));
                            }
                        }
                    }
                    return MapResult::err_str(&format!(
                        "{cid} dialer can't dial without an address",
                    ));
                }
                None => {
                    if let Some(configured_dial_a) = &self.fixed_target_addr {
                        return Dialer::dial_addr(configured_dial_a, params.a, params.b).await;
                    }
                    return MapResult::err_str(&format!(
                        "{cid} dialer can't dial without an address",
                    ));
                }
            },

            _ => {
                return MapResult::err_str(&format!(
                    "{cid} dialer can't dial when a stream already exists"
                ))
            }
        }
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
    ) -> anyhow::Result<Receiver<MapResult>> {
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
            Err(e) => {
                let mut e: anyhow::Error = e.into();
                e = e.context(format!("listen {} failed", a));
                Err(e)
            }
        }
    }

    /// not recommended, use listen_addr
    pub async fn listen_addr_forever(a: &net::Addr) -> anyhow::Result<Receiver<MapResult>> {
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
            Err(e) => Err(e.into()),
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
            Ok(rx) => MapResult::builder().c(Stream::g(rx)).build(),
            Err(e) => MapResult::from_e(e),
        }
    }
}
