pub mod accept;
pub mod echo;

use macro_mapper::{mapper_ext_fields, MapperExt, NoMapperExt};
use tokio::sync::mpsc::Receiver;
use tracing::debug;
use tracing::info;

use super::*;
use crate::map;
use crate::Name;

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
            info!(cid = %cid, " consumed by blackhole");
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

        if tracing::enabled!(tracing::Level::DEBUG) {
            let buf = params.b.as_ref().map(|b| b.len());
            debug!(
                cid = %cid,
                addr = %a,
                behavior = ?behavior,
                buf = ?buf,
                "direct dial",

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
                if matches!(stream, Stream::Conn(_))
                    && self.is_tail_of_chain()
                    && params.b.is_some()
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

pub fn get_addr_from_vvd(vd: Vec<Option<Box<dyn Data>>>) -> Option<net::Addr> {
    for vd in vd.iter().flatten() {
        let ad = vd.get_raddr();
        if ad.is_some() {
            return ad;
        }
    }
    None
}

#[async_trait]
impl Mapper for Dialer {
    /// try the parameter first, if no addr was given, use configured addr.
    /// 注意, dial addr 和target addr (params.a) 不一样
    async fn maps(&self, _cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::None => {
                let vd = params.d;
                let d = get_addr_from_vvd(vd);
                match d {
                    Some(a) => {
                        return Dialer::dial_addr(&a, params.a, params.b).await;
                    }

                    None => {
                        if let Some(configured_dial_a) = &self.configured_target_addr() {
                            return Dialer::dial_addr(configured_dial_a, params.a, params.b).await;
                        }
                        return MapResult::err_str(&format!(
                            "Dialer can't dial without an address",
                        ));
                    }
                }
            }

            _ => {
                return MapResult::err_str(&format!(
                    "Dialer can't dial when a stream already exists"
                ))
            }
        }
    }
}

/// Listener can listen tcp and unix domain socket
#[mapper_ext_fields]
#[derive(MapperExt, Clone, Debug, Default)]
pub struct Listener {}

impl Name for Listener {
    fn name(&self) -> &'static str {
        "listener"
    }
}
impl Listener {
    pub async fn listen_addr(
        a: &net::Addr,
        shutdown_rx: oneshot::Receiver<()>,
    ) -> anyhow::Result<Receiver<MapResult>> {
        let listener = match listen::listen(a).await {
            Ok(l) => l,
            Err(e) => return Err(e.context(format!("Listener failed for {}", a))),
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
impl Mapper for Listener {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let a = match params.a.as_ref() {
            Some(a) => a,
            None => self
                .configured_target_addr()
                .as_ref()
                .expect("Listener always has a fixed_target_addr"),
        };

        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!(cid = %cid, addr = %a, "start listen")
        }

        let r = match params.shutdown_rx {
            Some(rx) => Listener::listen_addr(a, rx).await,
            None => Listener::listen_addr_forever(a).await,
        };

        match r {
            Ok(rx) => MapResult::builder().c(Stream::g(rx)).build(),
            Err(e) => MapResult::from_e(e),
        }
    }
}
