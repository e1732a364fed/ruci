/*!
similar to ruci::map::network, but with SockOpt
 */
use async_trait::async_trait;
use bytes::BytesMut;
use macro_map::*;
use ruci::map::network::accept;
use ruci::map::{self, *};
use ruci::net::*;
use ruci::*;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Receiver;
use tokio::sync::oneshot;
use tracing::debug;

use crate::net::so2::{self, SockOpt};

/// Listener can listen tcp, with sock_opt
#[map_ext_fields]
#[derive(MapExt, Clone, Debug, Default)]
pub struct TcpOptListener {
    pub listen_addr: net::Addr,
    pub sopt: SockOpt,
}

impl Name for TcpOptListener {
    fn name(&self) -> &'static str {
        "tcp_opt_listener"
    }
}
impl TcpOptListener {
    pub async fn listen_addr(
        &self,
        a: &net::Addr,
        shutdown_rx: oneshot::Receiver<()>,
        opt_fixed_target_addr: Option<net::Addr>,
    ) -> anyhow::Result<Receiver<MapResult>> {
        let listener = match so2::listen_tcp(a, &self.sopt) {
            Ok(l) => l,
            Err(e) => return Err(e.context(format!("tcp_opt_listener failed for {}", a))),
        };

        let listener = ruci::net::listen::Listener::TCP(listener);

        let r = accept::loop_accept(listener, shutdown_rx, opt_fixed_target_addr).await;

        Ok(r)
    }

    /// not recommended, use listen_addr
    pub async fn listen_addr_forever(
        &self,
        a: &net::Addr,
        opt_fixed_target_addr: Option<net::Addr>,
    ) -> anyhow::Result<Receiver<MapResult>> {
        let listener = so2::listen_tcp(a, &self.sopt)?;
        let listener = ruci::net::listen::Listener::TCP(listener);

        let r = accept::loop_accept_forever(listener, opt_fixed_target_addr).await;

        Ok(r)
    }
}

#[async_trait]
impl Map for TcpOptListener {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let a = match params.a.as_ref() {
            Some(a) => a,
            None => &self.listen_addr,
        };

        let opt_fixed_target_addr = self.configured_target_addr().cloned();

        let r = match params.shutdown_rx {
            Some(rx) => self.listen_addr(a, rx, opt_fixed_target_addr).await,
            None => self.listen_addr_forever(a, opt_fixed_target_addr).await,
        };

        match r {
            Ok(rx) => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!(cid = %cid,addr= %a, "tcp_opt_listener started listen")
                }
                MapResult::builder().c(Stream::g(rx)).build()
            }
            Err(e) => MapResult::from_e(e),
        }
    }
}

/// Dial the given addr and optionaly set sockopt
///
/// Note:
/// dial udp by OptDirect won't ever timeout
#[map_ext_fields]
#[derive(Clone, Debug, Default, MapExt)]
pub struct OptDirect {
    pub sopt: SockOpt,
}
impl Name for OptDirect {
    fn name(&self) -> &'static str {
        "opt_direct"
    }
}

#[async_trait]
impl Map for OptDirect {
    /// dial params.a.
    async fn maps(&self, cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let a = match params.a {
            Some(a) => a,
            None => {
                return MapResult::err_str(&format!("{}, opt_direct need params.a, got empty", cid))
            }
        };

        if tracing::enabled!(tracing::Level::DEBUG) {
            let buf = params.b.as_ref().map(|b| b.len());
            debug!(
                cid = %cid,
                addr = %a,
                behavior = ?behavior,
                buf = ?buf,
                "opt_direct dial",

            );
        }

        let dial_r: anyhow::Result<Stream> = match behavior {
            ProxyBehavior::ENCODE => match a.network {
                Network::UDP => so2::dial_udp(&a, &self.sopt)
                    .map(|s| Stream::AddrConn(ruci::net::udp::new(s, None, false))),
                Network::TCP => so2::dial_tcp(&a, &self.sopt).map(|s| Stream::Conn(Box::new(s))),
                _ => todo!(),
            },
            _ => todo!(),
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
                        e = e.context("opt_direct try write early data");
                        return MapResult::from_e(e);
                    }
                    return MapResult::builder().c(stream).build();
                }
                match stream {
                    Stream::Conn(_) => return MapResult::builder().c(stream).b(params.b).build(),
                    Stream::AddrConn(_) => {
                        return MapResult::builder()
                            .c(stream)
                            .b(params.b)
                            .a(Some(a))
                            .no_timeout(true)
                            .build()
                    }
                    Stream::Generator(_) => todo!(),
                    Stream::None => todo!(),
                }
            }
            Err(e) => return MapResult::from_e(e.context(format!("opt_direct dial {} failed", a))),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OptDialerOption {
    pub dial_addr: String,
    pub sockopt: crate::net::so2::SockOpt,
}

/// Dial the pre-set addr and optionaly set sockopt,
/// pass on the params.a
#[map_ext_fields]
#[derive(Clone, Debug, Default, MapExt)]
pub struct OptDialer {
    pub sockopt: SockOpt,
    pub dial_addr: net::Addr,
}

impl Name for OptDialer {
    fn name(&self) -> &'static str {
        "opt_dialer"
    }
}

impl OptDialer {
    pub fn new(opt: OptDialerOption) -> anyhow::Result<Self> {
        Ok(Self {
            dial_addr: net::Addr::from_network_addr_url(&opt.dial_addr)?,
            sockopt: opt.sockopt,
            ext_fields: Some(MapExtFields::default()),
        })
    }
    pub async fn dial_addr(
        &self,
        dial_a: &net::Addr,
        pass_a: Option<net::Addr>,
        pass_b: Option<BytesMut>,
    ) -> MapResult {
        //debug!("start dial");
        let r = match dial_a.network {
            Network::UDP => so2::dial_udp(&dial_a, &self.sockopt)
                .map(|s| Stream::AddrConn(ruci::net::udp::new(s, None, false))),
            Network::TCP => {
                so2::dial_tcp(&dial_a, &self.sockopt).map(|s| Stream::Conn(Box::new(s)))
            }
            _ => todo!(),
        };

        //debug!("  dial r {:?}", r);

        match r {
            Ok(c) => MapResult::builder().c(c).a(pass_a).b(pass_b).build(),
            Err(e) => MapResult::from_e(e.context(format!("BindDialer dial {} failed", dial_a))),
        }
    }
}

#[async_trait]
impl Map for OptDialer {
    /// use configured addr.
    /// 注意, dial addr 和target addr (params.a) 不一样
    async fn maps(&self, _cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::None => {
                return self.dial_addr(&self.dial_addr, params.a, params.b).await;
            }

            _ => return MapResult::err_str("BindDialer can't dial when a stream already exists"),
        }
    }
}
