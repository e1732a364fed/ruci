/*!
similar to ruci::map::network, but with SockOpt
 */
use async_trait::async_trait;
use bytes::BytesMut;
use macro_mapper::*;
use ruci::map::network::accept;
use ruci::map::{self, *};
use ruci::net::*;
use ruci::*;
use tokio::sync::mpsc::Receiver;
use tokio::sync::oneshot;
use tracing::debug;

use crate::net::so2::{self, SockOpt};

/// Listener can listen tcp, with sock_opt
#[mapper_ext_fields]
#[derive(MapperExt, Clone, Debug, Default)]
pub struct TcpOptListener {
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
    ) -> anyhow::Result<Receiver<MapResult>> {
        let listener = match so2::listen_tcp(a, &self.sopt).await {
            Ok(l) => l,
            Err(e) => return Err(e.context(format!("tcp_opt_listener failed for {}", a))),
        };

        let listener = ruci::net::listen::Listener::TCP(listener);

        let r = accept::loop_accept(listener, shutdown_rx).await;

        Ok(r)
    }

    /// not recommended, use listen_addr
    pub async fn listen_addr_forever(&self, a: &net::Addr) -> anyhow::Result<Receiver<MapResult>> {
        let listener = so2::listen_tcp(a, &self.sopt).await?;
        let listener = ruci::net::listen::Listener::TCP(listener);

        let r = accept::loop_accept_forever(listener).await;

        Ok(r)
    }
}

#[async_trait]
impl Mapper for TcpOptListener {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let a = match params.a.as_ref() {
            Some(a) => a,
            None => self
                .configured_target_addr()
                .as_ref()
                .expect("tcp_opt_listener always has a fixed_target_addr"),
        };

        let r = match params.shutdown_rx {
            Some(rx) => self.listen_addr(a, rx).await,
            None => self.listen_addr_forever(a).await,
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

#[mapper_ext_fields]
#[derive(Clone, Debug, Default, MapperExt)]
pub struct OptDirect {
    pub sopt: SockOpt,
}
impl Name for OptDirect {
    fn name(&self) -> &'static str {
        "opt_direct"
    }
}

#[async_trait]
impl Mapper for OptDirect {
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
                    .await
                    .map(|s| Stream::AddrConn(ruci::net::udp::new(s, None))),
                Network::TCP => so2::dial_tcp(&a, &self.sopt)
                    .await
                    .map(|s| Stream::Conn(Box::new(s))),
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
                return MapResult::builder().c(stream).b(params.b).build();
            }
            Err(e) => return MapResult::from_e(e.context(format!("opt_direct dial {} failed", a))),
        }
    }
}

#[mapper_ext_fields]
#[derive(Clone, Debug, Default, MapperExt)]
pub struct OptDialer {
    pub sopt: SockOpt,
}

impl Name for OptDialer {
    fn name(&self) -> &'static str {
        "opt_dialer"
    }
}

impl OptDialer {
    pub async fn dial_addr(
        &self,
        dial_a: &net::Addr,
        pass_a: Option<net::Addr>,
        pass_b: Option<BytesMut>,
    ) -> MapResult {
        let r = match dial_a.network {
            Network::UDP => so2::dial_udp(&dial_a, &self.sopt)
                .await
                .map(|s| Stream::AddrConn(ruci::net::udp::new(s, None))),
            Network::TCP => so2::dial_tcp(&dial_a, &self.sopt)
                .await
                .map(|s| Stream::Conn(Box::new(s))),
            _ => todo!(),
        };

        match r {
            Ok(c) => MapResult::builder().c(c).a(pass_a).b(pass_b).build(),
            Err(e) => MapResult::from_e(e.context(format!("Dialer dial {} failed", dial_a))),
        }
    }
}

#[async_trait]
impl Mapper for OptDialer {
    /// use configured addr.
    /// 注意, dial addr 和target addr (params.a) 不一样
    async fn maps(&self, _cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::None => {
                if let Some(configured_dial_a) = &self.configured_target_addr() {
                    return self.dial_addr(configured_dial_a, params.a, params.b).await;
                }
                return MapResult::err_str("Dialer can't dial without an address");
            }

            _ => return MapResult::err_str("Dialer can't dial when a stream already exists"),
        }
    }
}
