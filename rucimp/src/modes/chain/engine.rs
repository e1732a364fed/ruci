use anyhow;
use futures::Future;
use log::{debug, info, warn};
use parking_lot::Mutex;
#[allow(unused)]
use ruci::net;
use ruci::{
    map::{
        acc::{DMIterBox, DynVecIterWrapper},
        *,
    },
    net::GlobalTrafficRecorder,
    relay::{handle_in_accumulate_result, route::*, *},
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{
    mpsc::{self, Receiver},
    oneshot::{self, Sender},
};

use super::config::StaticConfig;

#[derive(Default)]
pub struct Engine {
    pub running: Arc<Mutex<Option<Vec<Sender<()>>>>>, //这里约定，所有对 engine的热更新都要先访问running的锁，若有值说明 is running
    pub ti: Arc<GlobalTrafficRecorder>,

    pub newconn_recorder: OptNewInfoSender,

    #[cfg(feature = "trace")]
    pub conn_info_updater: net::OptUpdater,

    inbounds: Vec<DMIterBox>,                   // 不为空
    outbounds: Arc<HashMap<String, DMIterBox>>, //不为空
    default_outbound: Option<DMIterBox>,        // init 后一定有值
    tag_routes: Option<HashMap<String, String>>,
}

impl Engine {
    /// 清空配置。reset 后 可以 接着调用 init
    pub async fn reset(&mut self) {
        let running = self.running.lock();

        if running.is_none() {
            self.inbounds.clear();
            self.outbounds = Arc::<HashMap<String, DMIterBox>>::default();
            self.default_outbound = None;
            self.tag_routes = None;
            self.ti = Arc::<GlobalTrafficRecorder>::default();
            info!("Engine is reset successful");
        } else {
            warn!("Engine is running, can't be reset");
        }
    }

    pub fn init_static(&mut self, sc: StaticConfig) {
        let inbounds = sc.get_inbounds();
        self.inbounds = inbounds
            .into_iter()
            .map(|v| {
                let inbound: Vec<_> = v.into_iter().map(|o| Arc::new(o)).collect();

                let x: DMIterBox = Box::new(DynVecIterWrapper(inbound.into_iter()));
                x
            })
            .collect();

        let (d, m) = sc.get_default_and_outbounds_map();
        self.default_outbound = Some(d);
        self.outbounds = Arc::new(m);
        self.tag_routes = sc.get_tag_route();
    }

    #[cfg(feature = "lua")]
    pub fn init_lua_static(&mut self, config_string: String) {
        use crate::modes::chain::config::lua;
        let sc = lua::load_static(&config_string).expect("has valid lua codes in the file content");
        self.init_static(sc)
    }

    #[cfg(feature = "lua")]
    pub fn init_lua_dynamic(&mut self, config_string: String) -> anyhow::Result<()> {
        use crate::modes::chain::config::lua;
        let (sc, ibs, default_o, ods) = lua::load_bounded_dynamic(config_string)?;
        self.inbounds = ibs;
        self.default_outbound = Some(default_o);
        self.outbounds = ods;
        self.tag_routes = sc.get_tag_route();
        Ok(())
    }

    pub fn inbounds_count(&self) -> usize {
        self.inbounds.len()
    }

    pub fn outbounds_count(&self) -> usize {
        self.outbounds.len()
    }

    /// non-blocking
    pub async fn run(&self) -> anyhow::Result<()> {
        self.start_with_tasks().await.map(|tasks| {
            for task in tasks {
                tokio::spawn(task.0);
                tokio::spawn(task.1);
            }
        })
    }

    /// blocking
    pub async fn block_run(
        &self,
    ) -> anyhow::Result<Vec<Result<anyhow::Result<()>, tokio::task::JoinError>>> {
        let mut hv = Vec::new();
        self.start_with_tasks().await.map(|tasks| {
            for task in tasks {
                hv.push(tokio::spawn(task.0));
                hv.push(tokio::spawn(task.1));
            }
        })?;
        let r = futures::future::join_all(hv).await;
        Ok(r)
    }

    pub async fn start_with_tasks(
        &self,
    ) -> anyhow::Result<
        Vec<(
            impl Future<Output = anyhow::Result<()>>,
            impl Future<Output = anyhow::Result<()>>,
        )>,
    > {
        let m = self.running.clone();
        let mut running = m.lock();
        if running.is_none() {
        } else {
            return Err(anyhow::anyhow!("already started!"));
        }
        if self.inbounds_count() == 0 {
            return Err(anyhow::anyhow!("no inbound"));
        }
        if self.outbounds_count() == 0 {
            return Err(anyhow::anyhow!("no outbound"));
        }

        let mut tasks = Vec::new();
        let mut shutdown_tx_vec = Vec::new();

        let out_selector = self.get_out_selector();

        self.inbounds.clone().into_iter().for_each(|miter| {
            let (tx, rx) = oneshot::channel();

            let (atx, arx) = mpsc::channel(100); //todo: change this

            let t1 = acc::accumulate_from_start(atx, rx, miter.clone(), Some(self.ti.clone()));

            let t2 = Engine::loop_a(
                arx,
                out_selector.clone(),
                self.ti.clone(),
                self.newconn_recorder.clone(),
                #[cfg(feature = "trace")]
                self.conn_info_updater.clone(),
            );

            tasks.push((t1, t2));
            shutdown_tx_vec.push(tx);
        });
        info!("chain engine will run with {} inbounds", tasks.len());

        *running = Some(shutdown_tx_vec);
        Ok(tasks)
    }

    async fn loop_a(
        mut arx: Receiver<acc::AccumulateResult>,
        out_selector: Arc<Box<dyn OutSelector>>,
        ti: Arc<GlobalTrafficRecorder>,
        conn_info_recorder: OptNewInfoSender,
        #[cfg(feature = "trace")] conn_info_updater: net::OptUpdater,
    ) -> anyhow::Result<()> {
        loop {
            let ar = arx.recv().await;
            if let Some(ar) = ar {
                tokio::spawn(handle_in_accumulate_result(
                    ar,
                    out_selector.clone(),
                    Some(ti.clone()),
                    conn_info_recorder.clone(),
                    #[cfg(feature = "trace")]
                    conn_info_updater.clone(),
                ));
            } else {
                break;
            }
        }
        Ok(())
    }

    fn get_out_selector(&self) -> Arc<Box<dyn OutSelector>> {
        if self.tag_routes.is_some() {
            self.get_tag_route_out_selector()
        } else {
            self.get_fixed_out_selector()
        }
    }
    fn get_tag_route_out_selector(&self) -> Arc<Box<dyn OutSelector>> {
        let t = TagOutSelector {
            outbounds_tag_route_map: self.tag_routes.clone().expect("has tag_routes"),
            outbounds_map: self.outbounds.clone(),
            default: self.default_outbound.clone().expect("has default_outbound"),
        };

        Arc::new(Box::new(t))
    }

    fn get_fixed_out_selector(&self) -> Arc<Box<dyn OutSelector>> {
        let ib = self.default_outbound.clone().expect("has default_outbound");
        let fixed_selector = FixedOutSelector { default: ib };

        Arc::new(Box::new(fixed_selector))
    }

    /// 停止所有的 server, 但并不清空配置。意味着可以stop后接着调用 run
    pub async fn stop(&self) {
        info!("chain engine: stop called");
        let mut running = self.running.lock();
        let opt = running.take();

        if let Some(v) = opt {
            let mut i = 0;
            v.into_iter().for_each(|shutdown_tx| {
                debug!("sending close signal to inbound {}", i);
                let _ = shutdown_tx.send(());
                i += 1;
            });
        }

        info!("chain engine stopped");
    }
}
