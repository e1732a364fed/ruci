use futures::Future;
use log::{debug, info};
use parking_lot::Mutex;
use ruci::{
    map::*,
    net::TransmissionInfo,
    relay::{
        conn::handle_in_accumulate_result,
        route::{FixedOutSelector, OutSelector, TagOutSelector},
    },
};
use std::{collections::HashMap, io, sync::Arc};
use tokio::sync::{
    mpsc::{self, Receiver},
    oneshot::{self, Sender},
};

use super::config::StaticConfig;

/// 静态引擎中 使用 StaticConfig 作为配置
#[derive(Default)]
pub struct StaticEngine {
    pub running: Arc<Mutex<Option<Vec<Sender<()>>>>>, //这里约定，所有对 engine的热更新都要先访问running的锁，若有值说明 is running
    pub ti: Arc<TransmissionInfo>,

    inbounds: Vec<MIterBox>, //Vec<Vec<Box<dyn MapperSync>>>,   // 不为空
    outbounds: Arc<HashMap<String, MIterBox>>, //不为空
    default_outbound: Option<MIterBox>, // init 后一定有值
    tag_routes: Option<HashMap<String, String>>,

    //cache of static mems, manualy release required
    fix_outselector_mem: Option<&'static FixedOutSelector>,
}

impl StaticEngine {
    pub fn init(&mut self, sc: StaticConfig) {
        let inbounds = sc.get_inbounds();
        self.inbounds = inbounds
            .into_iter()
            .map(|v| {
                let v = Box::leak(Box::new(v));
                let v = v.iter();

                let x: MIterBox = Box::new(v);
                x
            })
            .collect();

        let (d, m) = sc.get_default_and_outbounds_map();
        self.default_outbound = Some(d);
        self.outbounds = Arc::new(m);
        self.tag_routes = sc.get_tag_route();
    }

    pub fn server_count(&self) -> usize {
        self.inbounds.len()
    }

    pub fn client_count(&self) -> usize {
        self.outbounds.len()
    }

    /// non-blocking
    pub async fn run(&'static mut self) -> io::Result<()> {
        self.start_with_tasks().await.map(|tasks| {
            for task in tasks {
                tokio::spawn(task.0);
                tokio::spawn(task.1);
            }
        })
    }

    /// blocking
    pub async fn block_run(
        &'static mut self,
    ) -> io::Result<Vec<Result<io::Result<()>, tokio::task::JoinError>>> {
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
        &'static mut self,
    ) -> std::io::Result<
        Vec<(
            impl Future<Output = Result<(), std::io::Error>>,
            impl Future<Output = Result<(), std::io::Error>>,
        )>,
    > {
        let m = self.running.clone();
        let mut running = m.lock();
        if running.is_none() {
        } else {
            return Err(io::Error::other("already started!"));
        }
        if self.server_count() == 0 {
            return Err(io::Error::other("no server"));
        }
        if self.client_count() == 0 {
            return Err(io::Error::other("no client"));
        }

        let mut tasks = Vec::new();
        let mut shutdown_tx_vec = Vec::new();

        let out_selector = self.get_out_selector();

        self.inbounds.iter().for_each(|inmappers| {
            let (tx, rx) = oneshot::channel();

            let (atx, arx) = mpsc::channel(100); //todo: change this

            let oti = self.ti.clone();
            let t1 = async {
                accumulate_from_start(atx, rx, inmappers.clone(), Some(oti)).await;
                Ok(())
            };

            let t2 = StaticEngine::loop_a(arx, out_selector, self.ti.clone());

            tasks.push((t1, t2));
            shutdown_tx_vec.push(tx);
        });
        debug!("engine will run with {} inbounds", tasks.len());

        *running = Some(shutdown_tx_vec);
        Ok(tasks)
    }

    async fn loop_a(
        mut arx: Receiver<AccumulateResult>,
        out_selector: &'static dyn OutSelector,
        ti: Arc<TransmissionInfo>,
    ) -> io::Result<()> {
        loop {
            let ar = arx.recv().await;
            if ar.is_none() {
                break;
            }
            let ar = ar.unwrap();
            tokio::spawn(handle_in_accumulate_result(
                ar,
                out_selector,
                Some(ti.clone()),
            ));
        }
        Ok(())
    }

    fn get_out_selector(&mut self) -> &'static dyn OutSelector {
        if self.tag_routes.is_some() {
            self.get_tag_route_out_selector()
        } else {
            self.get_fixed_out_selector()
        }
    }
    fn get_tag_route_out_selector(&mut self) -> &'static dyn OutSelector {
        let t = TagOutSelector {
            outbounds_tag_route_map: self.tag_routes.clone().unwrap(),
            outbounds_map: self.outbounds.clone(),
            default: self.default_outbound.clone().unwrap(),
        };

        //todo: do the same as try_drop_fixed_selector
        let t = Box::leak(Box::new(t));
        t
    }

    fn get_fixed_out_selector(&mut self) -> &'static dyn OutSelector {
        let ib = self.default_outbound.clone().unwrap();
        let fixed_selector = FixedOutSelector { default: ib };
        let fixed_selector = Box::new(fixed_selector);
        let fixed_selector: &'static FixedOutSelector = Box::leak(fixed_selector);

        self.try_drop_fixed_selector();
        self.fix_outselector_mem = Some(fixed_selector);
        fixed_selector
    }

    fn try_drop_fixed_selector(&self) {
        match self.fix_outselector_mem {
            Some(exist_mem) => unsafe {
                let boxed =
                    Box::from_raw(exist_mem as *const FixedOutSelector as *mut FixedOutSelector);

                std::mem::drop(boxed);
            },
            None => {}
        }
    }

    /// 清空配置。reset 后 可以 接着调用 init
    pub async fn reset(&self) {
        //todo: 处理 leak
        self.try_drop_fixed_selector();
    }

    /// 停止所有的 server, 但并不清空配置。意味着可以stop后接着调用 run
    pub async fn stop(&self) {
        info!("stop called");
        let mut running = self.running.lock();
        let opt = running.take();

        if let Some(v) = opt {
            let mut i = 0;
            v.into_iter().for_each(|shutdown_tx| {
                debug!("sending close signal to listener {}", i);
                let _ = shutdown_tx.send(());
                i += 1;
            });
        }

        info!("stopped");
    }
}
