/*!
Defines [`Map`]s that can either generate basic Stream(s) like ip/tcp/udp/uds, or consume a Stream.
*/

pub mod accept;

use macro_map::*;
use tokio::sync::mpsc::Receiver;
use tracing::debug;
use tracing::info;

use super::*;
use crate::map;
use crate::Name;
use anyhow::Result;

/// BlackHole drops the connection instantly
#[map_ext_fields]
#[derive(MapExt, Debug, Default, Clone)]
pub struct BlackHole {}

impl Name for BlackHole {
    fn name(&self) -> &str {
        "blackhole"
    }
}

impl BlackHole {
    pub fn boxed() -> MapBox {
        Box::<BlackHole>::default()
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
pub struct Direct {
    pub leak_target_addr: bool,
    pub opt_dns_client: Option<Arc<dns::AsyncClient>>,
}
impl Name for Direct {
    fn name(&self) -> &'static str {
        "direct"
    }
}

#[async_trait]
impl Map for Direct {
    /// dial params.a.
    async fn maps(&self, cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let dial_a = match params.a {
            Some(a) => a,
            None => {
                return MapResult::from_err_str(&format!(
                    "{}, direct need params.a, got empty",
                    cid
                ))
            }
        };

        if tracing::enabled!(tracing::Level::DEBUG) {
            let buf = params.b.as_ref().map(|b| b.len());
            debug!(
                cid = %cid,
                addr = %dial_a,
                behavior = ?behavior,
                buf = ?buf,
                "direct dial",

            );
        }

        let dial_r = match behavior {
            ProxyBehavior::ENCODE => match dial_a.network {
                Network::UDP => dial_a.try_dial_udp(self.opt_dns_client.clone()).await,
                _ => dial_a.try_dial(self.opt_dns_client.clone()).await,
            },
            _ => dial_a.try_dial(self.opt_dns_client.clone()).await,
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

                    let builder = MapResult::builder().c(stream);
                    return if self.leak_target_addr {
                        builder.a(Some(dial_a)).build()
                    } else {
                        builder.build()
                    };
                }

                let builder = MapResult::builder().c(stream);
                if self.leak_target_addr {
                    builder.a(Some(dial_a)).b(params.b).build()
                } else {
                    builder.b(params.b).build()
                }
            }
            Err(e) => {
                return MapResult::from_e(e.context(format!("Direct dial {} failed", dial_a)))
            }
        }
    }
}

#[cfg(feature = "tun")]
#[derive(Clone, Debug, Default)]
enum AutoRouteState {
    #[default]
    None,
    InUp(Option<Vec<String>>), //old_dns_list
    OutUp,
    Down,
}

/// BindDialer can dial ip, tcp, udp or unix domain socket
#[map_ext_fields]
#[derive(Clone, Debug, Default, MapExt)]
pub struct BindDialer {
    pub dial_addr: Option<net::Addr>,
    pub bind_addr: Option<net::Addr>,
    pub opt_dns_client: Option<Arc<dns::AsyncClient>>,

    #[cfg(feature = "tun")]
    pub in_auto_route: Option<tun::route::InAutoRouteParams>,

    #[cfg(feature = "tun")]
    pub out_auto_route: Option<tun::route::OutAutoRouteParams>,

    #[cfg(feature = "tun")]
    auto_route_state: Arc<parking_lot::Mutex<AutoRouteState>>,
}

impl Name for BindDialer {
    fn name(&self) -> &'static str {
        "bind_dialer"
    }
}

#[cfg(feature = "tun")]
impl Drop for BindDialer {
    fn drop(&mut self) {
        self.down_route();
    }
}

impl BindDialer {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(feature = "tun")]
    pub fn down_route(&mut self) {
        let mut mg = self.auto_route_state.lock();
        match &*mg {
            AutoRouteState::InUp(opt_dns_list) => {
                debug!("BindDialer down in auto route");

                let mut params = self.in_auto_route.clone().unwrap();
                params.dns_list = opt_dns_list.to_owned();
                let r = tun::route::in_down_route(&params);
                debug!("BindDialer down in auto route {r:?}");
                if r.is_ok() {
                    *mg = AutoRouteState::Down;
                }
            }
            AutoRouteState::OutUp => {
                debug!("BindDialer down out auto route");

                let params = self.out_auto_route.clone().unwrap();
                let r = tun::route::out_down_route(&params);
                debug!("BindDialer down out auto route {r:?}");
                if r.is_ok() {
                    *mg = AutoRouteState::Down;
                }
            }
            _ => {}
        }
    }
    pub async fn action(
        &self,
        bind_a: Option<&net::Addr>,
        dial_a: Option<&net::Addr>,

        pass_a: Option<net::Addr>,
        pass_b: Option<BytesMut>,
        udp_fix_target_listen: Option<bool>,

        pass_shutdown_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    ) -> MapResult {
        let r = net::Addr::bind_dial(
            bind_a,
            dial_a,
            udp_fix_target_listen,
            self.opt_dns_client.clone(),
        )
        .await;

        match r {
            Ok(c) => {
                #[cfg(feature = "tun")]
                if let Some(a) = &bind_a {
                    if let Network::IP = a.network {
                        if let Some(c) = &self.in_auto_route {
                            let mut mg = self.auto_route_state.lock();
                            match &*mg {
                                AutoRouteState::InUp(_) => {
                                    info!("BindDialer called after AutoRouteState::InUp")
                                }
                                _ => {
                                    let r = tun::route::in_auto_route(c);
                                    match r {
                                        Ok(opt_dns_list) => {
                                            *mg = AutoRouteState::InUp(opt_dns_list);
                                        }
                                        Err(e) => {
                                            return MapResult::from_e(
                                                e.context("BindDialer in auto_route failed"),
                                            )
                                        }
                                    }
                                }
                            }
                        } else if let Some(c) = &self.out_auto_route {
                            let mut mg = self.auto_route_state.lock();
                            match &*mg {
                                AutoRouteState::OutUp => {
                                    info!("BindDialer called after AutoRouteState::OutUp")
                                }
                                _ => {
                                    let r = tun::route::out_auto_route(c);
                                    match r {
                                        Ok(_) => {
                                            *mg = AutoRouteState::OutUp;
                                        }
                                        Err(e) => {
                                            return MapResult::from_e(
                                                e.context("BindDialer out auto_route failed"),
                                            )
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                match pass_shutdown_rx {
                    Some(s) => MapResult::builder()
                        .c(c)
                        .a(pass_a)
                        .b(pass_b)
                        .shutdown_rx(s)
                        .build(),
                    None => MapResult::builder().c(c).a(pass_a).b(pass_b).build(),
                }
            }
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
                        return self
                            .action(
                                self.bind_addr.as_ref(),
                                Some(&a),
                                target_addr,
                                params.b,
                                udp_fix_target_listen,
                                params.shutdown_rx,
                            )
                            .await;
                    }

                    None => {
                        return self
                            .action(
                                self.bind_addr.as_ref(),
                                self.dial_addr.as_ref(),
                                target_addr,
                                params.b,
                                udp_fix_target_listen,
                                params.shutdown_rx,
                            )
                            .await;
                    }
                }
            }

            _ => {
                return MapResult::from_err_str("BindDialer can't map when a stream already exists")
            }
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
    ) -> Result<Receiver<MapResult>> {
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
    ) -> Result<Receiver<MapResult>> {
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
