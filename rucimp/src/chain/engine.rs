use futures::Future;
use log::{debug, info};
use parking_lot::Mutex;
use ruci::{
    map::{acc2::MIterBox, *},
    net::TransmissionInfo,
    relay::{
        conn2::handle_in_accumulate_result,
        route2::{FixedOutSelector, OutSelector, TagOutSelector},
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

    inbounds: Vec<MIterBox>,                   // 不为空
    outbounds: Arc<HashMap<String, MIterBox>>, //不为空
    default_outbound: Option<MIterBox>,        // init 后一定有值
    tag_routes: Option<HashMap<String, String>>,
}

impl StaticEngine {
    pub fn init(&mut self, sc: StaticConfig) {
        let inbounds = sc.get_inbounds();
        self.inbounds = inbounds
            .into_iter()
            .map(|v| {
                let inbound: Vec<_> = v.into_iter().map(|o| Arc::new(o)).collect();

                let x: MIterBox = Box::new(inbound.into_iter());
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
                acc2::accumulate_from_start(atx, rx, inmappers.clone(), Some(oti)).await;
                Ok(())
            };

            let t2 = StaticEngine::loop_a(arx, out_selector.clone(), self.ti.clone());

            tasks.push((t1, t2));
            shutdown_tx_vec.push(tx);
        });
        debug!("engine will run with {} inbounds", tasks.len());

        *running = Some(shutdown_tx_vec);
        Ok(tasks)
    }

    async fn loop_a(
        mut arx: Receiver<acc2::AccumulateResult>,
        out_selector: Arc<Box<dyn OutSelector>>,
        ti: Arc<TransmissionInfo>,
    ) -> io::Result<()> {
        loop {
            let ar = arx.recv().await;
            if let Some(ar) = ar {
                tokio::spawn(handle_in_accumulate_result(
                    ar,
                    out_selector.clone(),
                    Some(ti.clone()),
                ));
            } else {
                break;
            }
        }
        Ok(())
    }

    fn get_out_selector(&mut self) -> Arc<Box<dyn OutSelector>> {
        if self.tag_routes.is_some() {
            self.get_tag_route_out_selector()
        } else {
            self.get_fixed_out_selector()
        }
    }
    fn get_tag_route_out_selector(&mut self) -> Arc<Box<dyn OutSelector>> {
        let t = TagOutSelector {
            outbounds_tag_route_map: self.tag_routes.clone().expect("has tag_routes"),
            outbounds_map: self.outbounds.clone(),
            default: self.default_outbound.clone().expect("has default_outbound"),
        };

        Arc::new(Box::new(t))
    }

    fn get_fixed_out_selector(&mut self) -> Arc<Box<dyn OutSelector>> {
        let ib = self.default_outbound.clone().expect("has default_outbound");
        let fixed_selector = FixedOutSelector { default: ib };

        Arc::new(Box::new(fixed_selector))
    }

    /// 清空配置。reset 后 可以 接着调用 init
    pub async fn reset(&self) {}

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
