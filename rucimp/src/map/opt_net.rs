/*! similar to ruci::map::network, but with SockOpt
 *
 */
use async_trait::async_trait;
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
    sopt: SockOpt,
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
            Err(e) => return Err(e.context(format!("Listener failed for {}", a))),
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
                .expect("Listener always has a fixed_target_addr"),
        };

        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("{}, start listen {}", cid, a)
        }

        let r = match params.shutdown_rx {
            Some(rx) => self.listen_addr(a, rx).await,
            None => self.listen_addr_forever(a).await,
        };

        match r {
            Ok(rx) => MapResult::builder().c(Stream::g(rx)).build(),
            Err(e) => MapResult::from_e(e),
        }
    }
}
