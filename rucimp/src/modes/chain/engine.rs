/*!
Defines the engine to run the chain config.
 */

use crate::route::{
    clash::ClashRuleOutSelector,
    geosite_gfw::{GeositeGfwConfig, GeositeGfwOutSelector},
};
use data_source::DataSource;

use super::config::StaticConfig;
use anyhow;
use futures::Future;
use parking_lot::Mutex;
#[allow(unused)]
use ruci::net;
use ruci::{
    map::{
        fold::{DMIterBox, DynVecIter},
        *,
    },
    net::{GlobalTrafficRecorder, CID},
    relay::{handle_in_fold_result, route::*, *},
};
use std::{collections::HashMap, sync::Arc, time};
use tokio::sync::{
    mpsc::{self, Receiver},
    oneshot::{self, Sender},
};
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

pub type InitEngineFn = Box<dyn Send + FnOnce(&mut Engine) -> anyhow::Result<()>>;

#[derive(Default)]
pub struct Engine {
    pub global_data: ruci::map::GlobalData,

    /// 存储关闭所有inbound 的 Sender
    ///
    ///  若有值说明 is running
    ///
    /// 约定, 所有对 engine的热更新都要先访问 此锁
    pub running: Arc<Mutex<Option<Vec<Sender<()>>>>>,
    pub gtr: Arc<GlobalTrafficRecorder>,

    pub new_conn_recorder: OptNewInfoSender,

    #[cfg(feature = "trace")]
    pub conn_info_updater: net::OptUpdater,

    /// 配置文件中有一些地方是指定文件名的，而 Engine 会从 data_source 中找到指定文件
    ///
    /// 这一项需要手动配置
    pub data_source: Arc<DataSource>,

    inbounds: Vec<DMIterBox>,                   // 不为空
    outbounds: Arc<HashMap<String, DMIterBox>>, //不为空
    default_outbound: Option<DMIterBox>,        // init 后一定有值
    tag_routes: Option<HashMap<String, String>>,
    fallback_routes: Option<HashMap<String, String>>,

    clash_rules: Option<Arc<clash_rules::ClashRuleMatcher>>,
    geosite_gfw: Option<GeositeGfwConfig>,
}

impl Engine {
    /// 每 new 一个 Engine 都会随机生成一个 global_data.run_instance_id
    pub fn new() -> Self {
        use rand::Rng;

        let mut rng = rand::rng();

        let run_instance_id = rng.random();

        info!("new Engine {run_instance_id}");

        Engine {
            global_data: GlobalData {
                run_instance_id,
                instance_start_time: Some(time::SystemTime::now()),
                ..Default::default()
            },
            data_source: crate::utils::default_file_source().into(),
            ..Default::default()
        }
    }

    pub fn set_default_file_source(&mut self) {
        self.set_file_source(crate::utils::default_file_source())
    }
    pub fn set_file_source(&mut self, fs: DataSource) {
        self.data_source = Arc::new(fs)
    }

    /// 清空配置. reset 后 可以 接着调用 init_*
    ///
    /// 不会清空 data_source
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

        self.clash_rules = sc.get_clash_route(self.data_source.clone());
        self.geosite_gfw = sc.routes.and_then(|r| r.smart);
    }

    pub fn init_static(&mut self, sc: StaticConfig) -> anyhow::Result<()> {
        use anyhow::Context;
        let inbounds = sc
            .get_inbounds(self.data_source.clone())
            .context("sc.get_inbounds failed")?;
        self.inbounds = inbounds
            .into_iter()
            .map(|v| {
                let inbound: Vec<_> = v.into_iter().map(Arc::new).collect();

                let dbox: DMIterBox = Box::new(DynVecIter(inbound.into_iter()));
                dbox
            })
            .collect();

        let (d, m) = sc.get_default_and_outbounds_map(self.data_source.clone())?;
        self.default_outbound = Some(d);
        self.outbounds = Arc::new(m);
        self.load_routes_from(sc);
        Ok(())
    }

    /// finite dynamic or static, depends on the content of the lua code
    #[cfg(any(feature = "lua", feature = "lua54"))]
    pub fn init_lua(&mut self, lua_text: String) -> anyhow::Result<()> {
        // use crate::modes::chain::config::lua;

        debug!("trying init_lua");

        self.init_lua_static(lua_text)

        // let r = lua::finite::is_finite_dynamic_available(&lua_text);
        // match r {
        //     Ok(_) => self.init_lua_finite_dynamic(lua_text),
        //     Err(_) => self.init_lua_static(lua_text),
        // }
    }

    /// load static chain
    #[cfg(any(feature = "lua", feature = "lua54"))]
    pub fn init_lua_static(&mut self, lua_text: String) -> anyhow::Result<()> {
        use crate::modes::chain::config::lua;
        use anyhow::Context;
        debug!("trying init_lua_static");

        let sc = lua::load_static(&lua_text, self.data_source.clone())
            .context("lua::load_static failed")?;
        self.init_static(sc).context("init_lua_static failed")?;
        Ok(())
    }

    // load finite dynamic chain
    // #[cfg(any(feature = "lua", feature = "lua54"))]
    // pub fn init_lua_finite_dynamic(&mut self, lua_text: String) -> anyhow::Result<()> {
    //     use anyhow::Context;

    //     info!("initializing lua finite dynamic");

    //     use crate::modes::chain::config::lua;
    //     let (sc, ibs, default_o, ods) =
    //         lua::finite::load_finite_dynamic(&lua_text, self.data_source.clone())
    //             .context("Engine::init_lua_finite_dynamic: lua::load_finite_dynamic failed")?;
    //     self.inbounds = ibs;
    //     self.default_outbound = Some(default_o);
    //     self.outbounds = ods;
    //     self.load_routes_from(sc);
    //     Ok(())
    // }

    /// load infinite dynamic chain
    #[cfg(any(feature = "lua", feature = "lua54"))]
    pub fn init_lua_infinite_dynamic(&mut self, lua_text: String) -> anyhow::Result<()> {
        use crate::modes::chain::config::{dynamic::IndexInfinite, lua};

        info!("initializing lua infinite dynamic");

        let g_maps = lua::infinite::load_infinite_io(&lua_text, self.data_source.clone())?;

        let gi = g_maps.0;
        let go = g_maps.1;

        self.inbounds = Vec::from_iter(gi.into_iter().map(|(tag, g)| {
            let g = IndexInfinite::new(tag, Box::new(g));
            let dmb: DMIterBox = Box::new(g);
            dmb
        }));

        let mut first_o: Option<DMIterBox> = None;

        let obs: HashMap<String, DMIterBox> = go
            .into_iter()
            .map(|(tag, g)| {
                let g = IndexInfinite::new(tag.clone(), Box::new(g));
                let dmb: DMIterBox = Box::new(g);
                if first_o.is_none() {
                    first_o = Some(dmb.clone());
                }
                (tag, dmb)
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
            let t1 = fold::fold_from_start(
                cid,
                Some(self.global_data.clone()),
                atx,
                rx,
                miter.clone(),
                Some(self.gtr.clone()),
            );
            index += 1;

            let t2 = Engine::loop_in_to_out(
                self.global_data.clone(),
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

    async fn loop_in_to_out(
        global_data: GlobalData,

        mut rx: Receiver<fold::FoldResult>,
        out_selector: Arc<dyn OutSelector>,
        gtr: Arc<GlobalTrafficRecorder>,
        conn_info_recorder: OptNewInfoSender,
        #[cfg(feature = "trace")] conn_info_updater: net::OptUpdater,
    ) -> anyhow::Result<()> {
        loop {
            let ar = rx.recv().await;
            if let Some(ar) = ar {
                tokio::spawn(handle_in_fold_result(
                    ar,
                    Some(global_data.clone()),
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

    fn get_out_selector(&self) -> Arc<dyn OutSelector> {
        let mut ms = MultipleOutSelector::default();
        if self.tag_routes.is_some() || self.fallback_routes.is_some() {
            debug!("use tag_routes");
            ms.selectors.push(self.get_tag_route_out_selector())
        }

        if let Some(c) = self.clash_rules.clone() {
            ms.selectors.push(Arc::new(ClashRuleOutSelector {
                matcher: c,
                outbounds_map: self.outbounds.clone(),
            }))
        }
        if let Some(config) = self.geosite_gfw.clone() {
            ms.selectors.push(Arc::new(GeositeGfwOutSelector {
                config,
                outbounds_map: self.outbounds.clone(),
            }))
        }
        ms.selectors.push(self.get_fixed_out_selector());
        Arc::new(ms)
    }

    fn get_tag_route_out_selector(&self) -> Arc<dyn OutSelector> {
        let s = TagOutSelector {
            outbounds_tag_route_map: self.tag_routes.clone(),
            fallback_tag_route_map: self.fallback_routes.clone(),
            outbounds_map: self.outbounds.clone(),
            // ok_default: Some(self.default_outbound.clone().expect("has default_outbound")),
            ..Default::default()
        };

        Arc::new(s)
    }

    /// fix to default_outbound
    fn get_fixed_out_selector(&self) -> Arc<dyn OutSelector> {
        let ib = self.default_outbound.clone().expect("has default_outbound");
        let s = FixedOutSelector { default: ib };

        Arc::new(s)
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

    /// A helper function to start an engine, run it until it got shutdown signal, then stop it.
    ///
    /// use init_fn to modify the engine before it runs.
    pub async fn new_and_run(init_fn: InitEngineFn) -> anyhow::Result<()> {
        let mut e = Engine::new();

        debug!(
            "will run_static_engine with instance_id: {}",
            e.global_data.run_instance_id
        );

        init_fn(&mut e)?;

        let mut js = e.run().await?;

        crate::utils::wait_close_sig().await?;

        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(3));
            println!("Force shutdown after 3 secs!"); //only println works at this point.
            std::process::exit(1);
        });

        e.stop().await;

        debug!("Waiting for join set");

        js.shutdown().await;

        debug!(
            "run_static_engine finished, instance_id: {}",
            e.global_data.run_instance_id
        );

        Ok(())
    }

    /// 阻塞运行Engine, 其运行结束后 会自动对 Engine 调用 reset
    ///
    /// if force_exit == true, the program will exit after 10 seconds.
    pub async fn run_with_close_rx(
        &mut self,
        close_rx: Option<mpsc::Receiver<()>>,
        force_exit: bool,
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        let mut js = self.run().await.context("run_engine got error")?;

        info!("started rucimp chain engine");

        match close_rx {
            Some(rx) => crate::utils::wait_close_sig_with_closer(rx).await?,
            None => crate::utils::wait_close_sig().await?,
        }

        if force_exit {
            std::thread::spawn(|| {
                const WAIT_SEC: u64 = 10;
                std::thread::sleep(time::Duration::from_secs(WAIT_SEC));
                tracing::warn!("Force shutdown after {WAIT_SEC} secs!");
                println!("Force shutdown after {WAIT_SEC} secs!");
                std::process::exit(1);
            });
        }

        self.stop().await;

        js.shutdown().await;

        self.reset().await;
        tracing::info!(
            "chain engine shutted down gracefully, {}",
            self.global_data.run_instance_id
        );

        Ok(())
    }

    /// A helper function to start an engine with a static config, run it until it got shutdown signal, then stop it.
    pub async fn new_and_run_static(sc: StaticConfig) -> anyhow::Result<()> {
        let f = move |e: &mut Engine| {
            let mut fs = crate::utils::default_file_source();
            fs.insert_current_working_dir()?;
            e.data_source = Arc::new(fs);

            e.init_static(sc)
        };

        Engine::new_and_run(Box::new(f)).await
    }
}
