/*!
Defines Mappper s that can generate a/some basic Stream, or can consume a Stream.
*/

pub mod accept;
pub mod echo;

use macro_map::*;
use tokio::sync::mpsc::Receiver;
use tracing::debug;
use tracing::info;

use super::*;
use crate::map;
use crate::Name;

/// BlackHole drops the connection instantly
#[map_ext_fields]
#[derive(MapExt, Debug, Default, Clone)]
pub struct BlackHole {}

impl Name for BlackHole {
    fn name(&self) -> &str {
        "blackhole"
    }
}

#[async_trait]
impl Map for BlackHole {
    /// always consume the stream, ignore all params.
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        if params.c.is_some() {
            info!(cid = %cid, " consumed by blackhole");
        }
        return MapResult::default();
    }
}

/// Direct dial target addr directly
///
/// # Note
///
///  only use [`MapExt`]'s is_tail_of_chain. won't use configured_target_addr;
/// if you want to set configured_target_addr, maybe you should use TcpDialer
#[map_ext_fields]
#[derive(Clone, Debug, Default, MapExt)]
pub struct Direct {}
impl Name for Direct {
    fn name(&self) -> &'static str {
        "direct"
    }
}

#[async_trait]
impl Map for Direct {
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

/// BindDialer can dial ip, tcp, udp or unix domain socket
#[map_ext_fields]
#[derive(Clone, Debug, Default, MapExt)]
pub struct BindDialer {
    pub dial_addr: Option<net::Addr>,
    pub bind_addr: Option<net::Addr>,
    pub auto_route: Option<bool>,
}

impl Name for BindDialer {
    fn name(&self) -> &'static str {
        "bind_dialer"
    }
}

impl BindDialer {
    pub async fn action(
        bind_a: Option<&net::Addr>,
        dial_a: Option<&net::Addr>,

        pass_a: Option<net::Addr>,
        pass_b: Option<BytesMut>,
        udp_fix_target_listen: Option<bool>,
    ) -> MapResult {
        let r = net::Addr::bind_dial(bind_a, dial_a, udp_fix_target_listen).await;

        match r {
            Ok(c) => MapResult::builder().c(c).a(pass_a).b(pass_b).build(),
            Err(e) => MapResult::from_e(
                e.context(format!("BindDialer dial {:?} {:?} failed", bind_a, dial_a)),
            ),
        }
    }
}

fn get_addr_from_vd(vd: Vec<Option<Box<dyn Data>>>) -> Option<net::Addr> {
    for vd in vd.iter().flatten() {
        let ad = vd.get_raddr();
        if ad.is_some() {
            return ad;
        }
    }
    None
}

#[async_trait]
impl Map for BindDialer {
    /// try the parameter first, if no addr was given, use dial_addr.
    /// 注意, dial addr 和target addr (params.a) 不一样
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::None => {
                let vd = params.d;
                let d = get_addr_from_vd(vd);

                let mut target_addr = params.a;

                let mut udp_fix_target_listen: Option<bool> = None;

                if self.configured_target_addr().is_some() {
                    target_addr = self.configured_target_addr().cloned();

                    debug!(cid = %cid,target_addr = ?target_addr, "BindDialer using fixed_target_addr");

                    udp_fix_target_listen = Some(false);
                }

                if let Some(a) = &self.bind_addr {
                    if let Network::UDP = a.network {
                        if udp_fix_target_listen.is_some() {
                            udp_fix_target_listen = Some(true)
                        }
                    }
                }

                match d {
                    Some(a) => {
                        return BindDialer::action(
                            self.bind_addr.as_ref(),
                            Some(&a),
                            target_addr,
                            params.b,
                            udp_fix_target_listen,
                        )
                        .await;
                    }

                    None => {
                        return BindDialer::action(
                            self.bind_addr.as_ref(),
                            self.dial_addr.as_ref(),
                            target_addr,
                            params.b,
                            udp_fix_target_listen,
                        )
                        .await;
                    }
                }
            }

            _ => return MapResult::err_str("BindDialer can't map when a stream already exists"),
        }
    }
}

/// Listener can listen tcp,udp and unix domain socket.
///
/// udp Listener is only supported with fixed_target_addr
#[map_ext_fields]
#[derive(MapExt, Clone, Debug, Default)]
pub struct Listener {
    pub listen_addr: net::Addr,
}

impl Name for Listener {
    fn name(&self) -> &'static str {
        "listener"
    }
}
impl Listener {
    pub async fn listen_addr(
        a: &net::Addr,
        shutdown_rx: oneshot::Receiver<()>,
        opt_fixed_target_addr: Option<net::Addr>,
    ) -> anyhow::Result<Receiver<MapResult>> {
        let listener = match listen::listen(a, opt_fixed_target_addr.clone()).await {
            Ok(l) => l,
            Err(e) => return Err(e.context(format!("Listener failed for {}", a))),
        };

        let r = accept::loop_accept(listener, shutdown_rx, opt_fixed_target_addr).await;

        Ok(r)
    }

    /// not recommended, use listen_addr
    pub async fn listen_addr_forever(
        a: &net::Addr,
        opt_fixed_target_addr: Option<net::Addr>,
    ) -> anyhow::Result<Receiver<MapResult>> {
        let listener = listen::listen(a, opt_fixed_target_addr.clone()).await?;

        let r = accept::loop_accept_forever(listener, opt_fixed_target_addr).await;

        Ok(r)
    }
}

#[async_trait]
impl Map for Listener {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let a = match params.a.as_ref() {
            Some(a) => a,
            None => &self.listen_addr,
        };

        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!(cid = %cid, addr = %a, "start listen")
        }
        let opt_fixed_target_addr = self.configured_target_addr().cloned();

        let r = match params.shutdown_rx {
            Some(rx) => Listener::listen_addr(a, rx, opt_fixed_target_addr).await,
            None => Listener::listen_addr_forever(a, opt_fixed_target_addr).await,
        };

        match r {
            Ok(rx) => MapResult::builder().c(Stream::g(rx)).build(),
            Err(e) => MapResult::from_e(e),
        }
    }
}
