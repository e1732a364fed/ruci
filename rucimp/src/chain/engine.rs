use futures::Future;
use log::{debug, info};
use parking_lot::Mutex;
use ruci::{
    map::*,
    net::TransmissionInfo,
    relay::{conn::handle_in_accumulate_result, route::FixedOutSelector},
};
use std::{io, sync::Arc};
use tokio::sync::{
    mpsc,
    oneshot::{self, Sender},
};

use super::config::StaticConfig;

/// 静态引擎中 使用 StaticConfig 作为配置
#[derive(Default)]
pub struct StaticEngine {
    pub running: Arc<Mutex<Option<Vec<Sender<()>>>>>, //这里约定，所有对 engine的热更新都要先访问running的锁
    pub ti: Arc<TransmissionInfo>,

    inbounds: Vec<Vec<Box<dyn MapperSync>>>,  //servers
    outbounds: Vec<Vec<Box<dyn MapperSync>>>, //clients
}

impl StaticEngine {
    pub fn init(&mut self, sc: StaticConfig) {
        self.inbounds = sc.get_inbounds();
        self.outbounds = sc.get_outbounds();
    }

    pub fn server_count(&self) -> usize {
        self.inbounds.len()
    }

    pub fn client_count(&self) -> usize {
        self.outbounds.len()
    }

    /// non-blocking
    pub async fn run(&'static self) -> io::Result<()> {
        self.start_with_tasks().await.map(|tasks| {
            for task in tasks {
                tokio::spawn(task.0);
                tokio::spawn(task.1);
            }
        })
    }

    /// blocking
    pub async fn block_run(
        &'static self,
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
        &'static self,
    ) -> std::io::Result<
        Vec<(
            impl Future<Output = Result<(), std::io::Error>>,
            impl Future<Output = Result<(), std::io::Error>>,
        )>,
    > {
        let mut running = self.running.lock();
        if let None = *running {
        } else {
            return Err(io::Error::other("already started!"));
        }
        if self.server_count() == 0 {
            return Err(io::Error::other("no server"));
        }
        if self.client_count() == 0 {
            return Err(io::Error::other("no client"));
        }

        let defaultc = self.outbounds.last().unwrap();

        //todo: 因为没实现路由功能，所以现在只能用 FixedOutSelector,返回第一个outbound

        let mut tasks = Vec::new();
        let mut shutdown_tx_vec = Vec::new();

        let it = defaultc.iter();
        let ib = Box::new(it);

        let fixed_selector = FixedOutSelector { mappers: ib };
        let fixed_selector = Box::new(fixed_selector);
        let fixed_selector: &'static FixedOutSelector = Box::leak(fixed_selector);

        self.inbounds.iter().for_each(|inmappers| {
            let (tx, rx) = oneshot::channel();

            let (atx, mut arx) = mpsc::channel(100); //todo: change this

            let oti = self.ti.clone();
            let t1 = async {
                let a = (*inmappers).clone();
                let a = Box::new(a);
                let a = Box::leak(a);

                let ait = a.iter();
                let aib = Box::new(ait);

                accumulate_from_start(atx, rx, aib, Some(oti)).await;
                Ok(())
            };

            let t2 = async move {
                loop {
                    let ar = arx.recv().await;
                    if ar.is_none() {
                        break;
                    }
                    let ar = ar.unwrap();
                    tokio::spawn(handle_in_accumulate_result(
                        ar,
                        fixed_selector,
                        Some(self.ti.clone()),
                    ));
                }
                Ok(())
            };

            tasks.push((t1, t2));
            shutdown_tx_vec.push(tx);
        });
        debug!("engine will run with {} inbounds", tasks.len());

        *running = Some(shutdown_tx_vec);
        return Ok(tasks);
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

        // let ss = self.servers.as_slice();
        // for s in ss {
        //     s.stop();
        // }
        info!("stopped");
    }
}
