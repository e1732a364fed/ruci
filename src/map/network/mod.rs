pub mod accept;
pub mod echo;

use log::{debug, info, log_enabled};
use macro_mapper::{mapper_ext_fields, MapperExt, NoMapperExt};
use tokio::sync::mpsc::Receiver;

use super::*;
use crate::Name;
use crate::{map, net::Addr};

#[derive(NoMapperExt, Debug, Default, Clone)]
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

/// only use MapperExt's is_tail_of_chain. won't use configured_target_addr;
/// if you want to set configured_target_addr, maybe you should use TcpDialer
#[mapper_ext_fields]
#[derive(Clone, Debug, Default, MapperExt)]
pub struct Direct {}
impl Name for Direct {
    fn name(&self) -> &'static str {
        "direct"
    }
}

#[async_trait]
impl Mapper for Direct {
    /// dial params.a.
    async fn maps(&self, cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let a = match params.a {
            Some(a) => a,
            None => {
                return MapResult::err_str(&format!("{}, direct need params.a, got empty", cid))
            }
        };

        if log_enabled!(log::Level::Debug) {
            debug!(
                "direct dial, {} , {}, {:?} {:?}",
                a,
                cid,
                behavior,
                params.b.as_ref().and_then(|b| Some(b.len()))
            );
        }

        let dial_r = match behavior {
            ProxyBehavior::ENCODE => match a.network {
                Network::UDP => a.try_dial_udp().await,
                _ => a.try_dial().await,
            },
            _ => a.try_dial().await,
        };
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
            Err(e) => return MapResult::from_e(e.context(format!("Direct dial {} failed", a))),
        }
    }
}

/// can dial tcp, udp or unix domain socket
#[mapper_ext_fields]
#[derive(Clone, Debug, Default, MapperExt)]
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
            Err(e) => MapResult::from_e(e.context(format!("Dialer dial {} failed", dial_a))),
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
                    if let Some(configured_dial_a) = &self.configured_target_addr() {
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

/// StreamGenerator can listen tcp and unix domain socket
#[mapper_ext_fields]
#[derive(MapperExt, Clone, Debug, Default)]
pub struct StreamGenerator {}

impl Name for StreamGenerator {
    fn name(&self) -> &'static str {
        "listener"
    }
}
impl StreamGenerator {
    pub async fn listen_addr(
        a: &net::Addr,
        shutdown_rx: oneshot::Receiver<()>,
    ) -> anyhow::Result<Receiver<MapResult>> {
        let listener = match listen::listen(a).await {
            Ok(l) => l,
            Err(e) => return Err(e.context(format!("listen failed for {}", a))),
        };

        let r = accept::loop_accept(listener, shutdown_rx).await;

        Ok(r)
    }

    /// not recommended, use listen_addr
    pub async fn listen_addr_forever(a: &net::Addr) -> anyhow::Result<Receiver<MapResult>> {
        let listener = listen::listen(a).await?;

        let r = accept::loop_accept_forever(listener).await;

        Ok(r)
    }
}

#[async_trait]
impl Mapper for StreamGenerator {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let a = match params.a.as_ref() {
            Some(a) => a,
            None => self
                .configured_target_addr()
                .as_ref()
                .expect("StreamGenerator always has a fixed_target_addr"),
        };

        if log_enabled!(log::Level::Debug) {
            debug!("{}, start listen tcp {}", cid, a)
        }

        let r = match params.shutdown_rx {
            Some(rx) => StreamGenerator::listen_addr(a, rx).await,
            None => StreamGenerator::listen_addr_forever(a).await,
        };

        match r {
            Ok(rx) => MapResult::builder().c(Stream::g(rx)).build(),
            Err(e) => MapResult::from_e(e),
        }
    }
}
