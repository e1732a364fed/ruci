#[cfg(feature = "route")]
use crate::route::{RuleSet, RuleSetOutSelector};

use super::config::StaticConfig;
use anyhow;
use futures::Future;
use parking_lot::Mutex;
#[allow(unused)]
use ruci::net;
use ruci::{
    map::{
        fold::{DMIterBox, DynVecIterWrapper},
        *,
    },
    net::{GlobalTrafficRecorder, CID},
    relay::{handle_in_fold_result, route::*, *},
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{
    mpsc::{self, Receiver},
    oneshot::{self, Sender},
};
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

#[derive(Default)]
pub struct Engine {
    /// 存储关闭所有inbound 的 Sender
    ///
    ///  若有值说明 is running
    pub running: Arc<Mutex<Option<Vec<Sender<()>>>>>, //这里约定, 所有对 engine的热更新都要先访问running的锁
    pub gtr: Arc<GlobalTrafficRecorder>,

    pub new_conn_recorder: OptNewInfoSender,

    #[cfg(feature = "trace")]
    pub conn_info_updater: net::OptUpdater,

    inbounds: Vec<DMIterBox>,                   // 不为空
    outbounds: Arc<HashMap<String, DMIterBox>>, //不为空
    default_outbound: Option<DMIterBox>,        // init 后一定有值
    tag_routes: Option<HashMap<String, String>>,
    fallback_routes: Option<HashMap<String, String>>,

    #[cfg(feature = "route")]
    rule_sets: Option<Vec<RuleSet>>,
}

impl Engine {
    /// 清空配置. reset 后 可以 接着调用 init
    pub async fn reset(&mut self) {
        debug!("Engine reset called");
        let running = self.running.lock();

        if running.is_none() {
            self.inbounds.clear();
            self.outbounds = Arc::<HashMap<String, DMIterBox>>::default();
            self.default_outbound = None;
            self.tag_routes = None;
            self.gtr = Arc::<GlobalTrafficRecorder>::default();
            info!("Engine reset successful");
        } else {
            warn!("Engine is running, can't be reset. Should call stop before reset.");
        }
    }

    pub fn load_routes_from(&mut self, sc: StaticConfig) {
        self.tag_routes = sc.get_tag_route();
        self.fallback_routes = sc.get_fallback_route();

        #[cfg(feature = "route")]
        {
            self.rule_sets = sc.get_rule_route();
        }
    }

    pub fn init_static(&mut self, sc: StaticConfig) {
        let inbounds = sc.get_inbounds();
        self.inbounds = inbounds
            .into_iter()
            .map(|v| {
                let inbound: Vec<_> = v.into_iter().map(Arc::new).collect();

                let x: DMIterBox = Box::new(DynVecIterWrapper(inbound.into_iter()));
                x
            })
            .collect();

        let (d, m) = sc.get_default_and_outbounds_map();
        self.default_outbound = Some(d);
        self.outbounds = Arc::new(m);
        self.load_routes_from(sc);
    }

    /// finite dynamic or static, depends on the content of the lua code
    #[cfg(any(feature = "lua", feature = "lua54"))]
    pub fn init_lua(&mut self, config_string: String) -> anyhow::Result<()> {
        use crate::modes::chain::config::lua;

        debug!("trying init_lua");

        let r = lua::is_finite_dynamic_available(&config_string);
        match r {
            Ok(_) => self.init_lua_finite_dynamic(config_string),
            Err(_) => self.init_lua_static(config_string),
        }
    }

    /// load static chain
    #[cfg(any(feature = "lua", feature = "lua54"))]
    pub fn init_lua_static(&mut self, config_string: String) -> anyhow::Result<()> {
        use crate::modes::chain::config::lua;
        use anyhow::Context;
        debug!("trying init_lua_static");

        let sc = lua::load_static(&config_string).context("init_lua_static failed")?;
        self.init_static(sc);
        Ok(())
    }

    /// load finite dynamic chain
    #[cfg(any(feature = "lua", feature = "lua54"))]
    pub fn init_lua_finite_dynamic(&mut self, config_string: String) -> anyhow::Result<()> {
        use anyhow::Context;

        info!("initializing lua finite dynamic");

        use crate::modes::chain::config::lua;
        let (sc, ibs, default_o, ods) = lua::load_finite_dynamic(&config_string)
            .context("Engine::init_lua_finite_dynamic: lua::load_finite_dynamic failed")?;
        self.inbounds = ibs;
        self.default_outbound = Some(default_o);
        self.outbounds = ods;
        self.load_routes_from(sc);
        Ok(())
    }

    /// load infinite dynamic chain
    #[cfg(any(feature = "lua", feature = "lua54"))]
    pub fn init_lua_infinite_dynamic(&mut self, config_string: String) -> anyhow::Result<()> {
        use crate::modes::chain::config::{dynamic::IndexInfinite, lua};

        info!("initializing lua infinite dynamic");

        let g_maps = lua::load_infinite_io(&config_string)?;

        let gi = g_maps.0;
        let go = g_maps.1;

        self.inbounds = Vec::from_iter(gi.into_iter().map(|(tag, g)| {
            let g = IndexInfinite::new(tag, Box::new(g));
            let x: DMIterBox = Box::new(g);
            x
        }));

        let mut first_o: Option<DMIterBox> = None;

        let obs: HashMap<String, DMIterBox> = go
            .into_iter()
            .map(|(tag, g)| {
                let g = IndexInfinite::new(tag.clone(), Box::new(g));
                let x: DMIterBox = Box::new(g);
                if first_o.is_none() {
                    first_o = Some(x.clone());
                }
                (tag, x)
            })
            .collect();

        self.outbounds = Arc::new(obs);
        self.default_outbound = first_o;
        Ok(())
    }

    pub fn inbounds_count(&self) -> usize {
        self.inbounds.len()
    }

    pub fn outbounds_count(&self) -> usize {
        self.outbounds.len()
    }

    /// non-blocking. it calls start_with_tasks
    pub async fn run(&self) -> anyhow::Result<JoinSet<anyhow::Result<()>>> {
        let mut set = JoinSet::new();
        self.start_with_tasks().await.map(|tasks| {
            for task in tasks {
                set.spawn(task.0);
                set.spawn(task.1);
            }
        })?;
        Ok(set)
    }

    /// blocking. it calls run
    pub async fn block_run(&self) -> anyhow::Result<Vec<anyhow::Result<()>>> {
        let mut set = self.run().await?;
        let mut hv = Vec::new();
        while let Some(res) = set.join_next().await {
            let r = res.unwrap();
            hv.push(r)
        }
        Ok(hv)
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

        // must not be 0
        let mut index = 1u32;

        self.inbounds.clone().into_iter().for_each(|miter| {
            let (tx, rx) = oneshot::channel();

            let (atx, arx) = mpsc::channel(100); //todo: change this

            let cid = CID::new(index);
            debug!(inbound_index = index, "fold_from_start");
            let t1 = fold::fold_from_start(cid, atx, rx, miter.clone(), Some(self.gtr.clone()));
            index += 1;

            let t2 = Engine::loop_a(
                arx,
                out_selector.clone(),
                self.gtr.clone(),
                self.new_conn_recorder.clone(),
                #[cfg(feature = "trace")]
                self.conn_info_updater.clone(),
            );

            tasks.push((t1, t2));
            shutdown_tx_vec.push(tx);
        });
        info!(inbounds_count = tasks.len(), "chain engine started",);

        *running = Some(shutdown_tx_vec);
        Ok(tasks)
    }

    async fn loop_a(
        mut arx: Receiver<fold::FoldResult>,
        out_selector: Arc<Box<dyn OutSelector>>,
        gtr: Arc<GlobalTrafficRecorder>,
        conn_info_recorder: OptNewInfoSender,
        #[cfg(feature = "trace")] conn_info_updater: net::OptUpdater,
    ) -> anyhow::Result<()> {
        loop {
            let ar = arx.recv().await;
            if let Some(ar) = ar {
                tokio::spawn(handle_in_fold_result(
                    ar,
                    out_selector.clone(),
                    Some(gtr.clone()),
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
        #[cfg(feature = "route")]
        {
            if self.rule_sets.is_some() {
                debug!("use rule_sets");
                self.get_rule_sets_out_selector()
            } else if self.tag_routes.is_some() || self.fallback_routes.is_some() {
                debug!("use tag_routes");

                self.get_tag_route_out_selector()
            } else {
                debug!("use fixed_out_selector");
                self.get_fixed_out_selector()
            }
        }
        #[cfg(not(feature = "route"))]
        {
            if self.tag_routes.is_some() {
                self.get_tag_route_out_selector()
            } else {
                self.get_fixed_out_selector()
            }
        }
    }

    #[cfg(feature = "route")]
    fn get_rule_sets_out_selector(&self) -> Arc<Box<dyn OutSelector>> {
        let s = RuleSetOutSelector {
            outbounds_rules_vec: self.rule_sets.clone().expect("has rule_sets"),
            outbounds_map: self.outbounds.clone(),
            default: self.default_outbound.clone().expect("has default_outbound"),
        };

        Arc::new(Box::new(s))
    }

    fn get_tag_route_out_selector(&self) -> Arc<Box<dyn OutSelector>> {
        let s = TagOutSelector {
            outbounds_tag_route_map: self.tag_routes.clone(),
            fallback_tag_route_map: self.fallback_routes.clone(),
            outbounds_map: self.outbounds.clone(),
            ok_default: Some(self.default_outbound.clone().expect("has default_outbound")),
            ..Default::default()
        };

        Arc::new(Box::new(s))
    }

    fn get_fixed_out_selector(&self) -> Arc<Box<dyn OutSelector>> {
        let ib = self.default_outbound.clone().expect("has default_outbound");
        let s = FixedOutSelector { default: ib };

        Arc::new(Box::new(s))
    }

    /// 停止所有的 server, 但并不清空配置. 意味着可以stop后接着调用 run/block_run
    pub async fn stop(&self) {
        info!("chain engine: stop called");
        let mut running = self.running.lock();
        let opt = running.take();

        if let Some(v) = opt {
            let mut i = 0;
            v.into_iter().for_each(|shutdown_tx| {
                debug!(inbound = i, "sending close signal");
                let _ = shutdown_tx.send(());
                i += 1;
            });
        }

        info!("chain engine stopped");
    }
}
