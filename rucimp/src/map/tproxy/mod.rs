/*!
Tproxy related Maps. Tproxy is shortcut for transparent proxy,

Only support linux
 */
pub mod route;
pub mod udp;

pub use route::*;

use async_trait::async_trait;
use itertools::Itertools;
use ruci::map::{self, *};
use ruci::{
    net::{self, *},
    Name,
};

use macro_map::{map_ext_fields, MapExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

use crate::net::so2::SockOpt;
use crate::utils::{run_command, sync_run_command_list_no_stop, sync_run_command_list_stop};

/// TproxyResolver 从 系统发来的 tproxy 相关的 连接
/// 解析出实际 target_addr
#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct TcpResolver {
    opts: Options,
}

impl Name for TcpResolver {
    fn name(&self) -> &'static str {
        "tproxy_tcp_resolver"
    }
}

impl TcpResolver {
    pub fn new(opts: Options) -> anyhow::Result<Self> {
        if opts.auto_route_enabled() {
            info!("tproxy run auto_route");

            anyhow::Context::context(run_auto_route(&opts), "run auto_route commands failed")?;

            if opts.route_ipv6.unwrap_or_default() {
                info!("tproxy run_tcp_route6");
                let r = run_auto_route6(&opts);
                if let Err(e) = r {
                    warn!("tproxy run run_tcp_route6 got error {e}")
                }
            }
        }

        Ok(Self {
            opts,
            ext_fields: Some(MapExtFields::default()),
        })
    }
}

impl Drop for TcpResolver {
    fn drop(&mut self) {
        if self.opts.auto_route_enabled() {
            info!("tproxy run down_auto_route");

            let r = down_auto_route(&self.opts);
            if let Err(e) = r {
                warn!("tproxy run down_auto_route got error {e}")
            }

            if self.opts.route_ipv6.unwrap_or_default() {
                let r = down_auto_route6(&self.opts);
                if let Err(e) = r {
                    warn!("tproxy run down_auto_route6 got error {e}")
                }
            }
        }
    }
}

fn get_laddr_from_vd(vd: Vec<Option<Box<dyn Data>>>) -> Option<ruci::net::Addr> {
    for vd in vd.iter().flatten() {
        let oa = vd.get_laddr();
        if oa.is_some() {
            return oa;
        }
    }
    None
}

#[async_trait]
impl Map for TcpResolver {
    /// TcpResolver only has decode behavior
    ///
    async fn maps(&self, _cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        if let ProxyBehavior::ENCODE = behavior {
            return MapResult::err_str("tproxy TcpResolver doesn't support ENCODE behavior");
        }
        match params.c {
            Stream::Conn(c) => {
                let oa = get_laddr_from_vd(params.d);

                if oa.is_none() {
                    return MapResult::err_str(
                        "tproxy TcpResolver needs data for local_addr, did't get it from the data.",
                    );
                }

                // laddr in tproxy is in fact target_addr
                MapResult::new_c(c).a(oa).b(params.b).build()
            }
            _ => MapResult::err_str(&format!(
                "tproxy TcpResolver needs a tcp stream, got {}",
                params.c
            )),
        }
    }
}

#[map_ext_fields]
#[derive(Clone, Debug, Default, MapExt)]
pub struct UDPListener {
    pub listen_addr: net::Addr,
    pub sopt: SockOpt,
}

impl Name for UDPListener {
    fn name(&self) -> &'static str {
        "tproxy_udp_listener"
    }
}

impl UDPListener {
    pub async fn start_listen(
        &self,
        cid: CID,
        listen_a: &net::Addr,
        mut shutdown_rx: oneshot::Receiver<()>,
    ) -> anyhow::Result<MapResult> {
        match listen_a.network {
            Network::UDP => {}
            _ => anyhow::bail!(
                "tproxy_udp_listener need dial udp, got {}",
                listen_a.network
            ),
        }

        use anyhow::Context;

        rlimit::prlimit(
            std::process::id().try_into().unwrap(),
            rlimit::Resource::NOFILE,
            Some((1024000, 1024000)),
            None,
        )
        .context("run rlimit::prlimit failed")?;

        let mut listener = udp::Listener::new(listen_a.clone(), self.sopt.clone()).await?;

        let (tx, rx) = mpsc::channel(100);

        tokio::spawn(async move {
            let mut count = 0;

            loop {
                tokio::select! {
                    _ = &mut shutdown_rx=>{
                        debug!("tproxy udp listener got shutdown_rx");
                        listener.shutdown();
                        break
                    }
                    r = listener.accept()=>{
                        match r {
                            Ok(accept_data) => {
                                count +=1;

                                let mut cidc = cid.clone();
                                cidc.push_num(count);

                                let mr = MapResult::new_u(accept_data.ac)
                                    .a(Some(accept_data.dst.clone()))
                                    .b(Some(accept_data.first_buf))
                                    .d(Some(Box::new(map::data::RLAddr(accept_data.dst, accept_data.src))))
                                    .new_id(cidc)
                                    .build();
                                let r = tx.send(mr).await;
                                if let Err(e) = r {
                                    debug!("tproxy udp listener accept tx.send got e: {e}");
                                    break;
                                }
                            }
                            Err(e) => {
                                debug!("tproxy udp listener accept got e: {e}");
                                break},
                        }
                    }
                } //select
            } //loop
        }); //spawn

        let s = Stream::Generator(rx);
        Ok(MapResult::builder().c(s).build())
    }
}

#[async_trait]
impl Map for UDPListener {
    /// use configured addr.
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::None => {
                let r = self.start_listen(cid,&self.listen_addr, params.shutdown_rx.expect("tproxy_udp_listener requires a shutdown_rx to support graceful shutdown")).await;
                match r {
                    Ok(r) => r,
                    Err(e) => MapResult::from_e(e),
                }
            }

            _ => {
                return MapResult::err_str(
                    "tproxy_udp_listener can't dial when a stream already exists",
                )
            }
        }
    }
}
