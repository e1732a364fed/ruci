use futures::Future;
use log::{debug, info};
use parking_lot::Mutex;
use ruci::{
    map::*,
    net::{TransmissionInfo, CID},
    relay::conn::handle_in_accumulate_result,
};
use std::{default, io, sync::Arc};
use tokio::{
    sync::{
        mpsc,
        oneshot::{self, Sender},
    },
    task,
};

use super::config::StaticConfig;

#[derive(Default)]
pub struct StaticEngine {
    pub running: Arc<Mutex<Option<Vec<Sender<()>>>>>, //这里约定，所有对 engine的热更新都要先访问running的锁
    pub ti: Arc<TransmissionInfo>,

    pub config: StaticConfig,

    servers: Vec<Vec<Box<dyn MapperSync>>>,
    clients: Vec<Vec<Box<dyn MapperSync>>>,
}

impl StaticEngine {
    pub fn init(&mut self) {
        self.servers = self.config.get_listens();
        self.clients = self.config.get_dials();
    }

    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /*
        pub async fn start_with_tasks(
            &self,
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

            let defaultc = self.clients.last().unwrap();

            //let defaultc = self.default_c.clone().unwrap();

            //todo: 因为没实现路由功能，所以现在只能用一个 client, 即 default client
            // 路由后，要传递给 listen_ser 一个路由表

            let mut tasks = Vec::new();
            let mut shutdown_tx_vec = Vec::new();

            let selector = FixedOutSelector {
                mappers: defaultc.iter(),
            };
            let selector = Box::new(selector);
            let selector = Box::leak(selector);

            self.servers.iter().for_each(|inmappers| {
                let (tx, rx) = oneshot::channel(); //todo: change this

                //in_iter_accumulate(CID::default(), rx, tx, inmappers, None);

                let (atx, arx) = mpsc::channel(100);

                let t1 = async {
                    accumulate_from_start::<_>(atx, rx, inmappers.iter());
                    Ok(())
                };

                let t2 = async {
                    loop {
                        let ar = arx.recv().await;
                        if ar.is_none() {
                            break;
                        }
                        let ar = ar.unwrap();
                        handle_in_accumulate_result(ar, selector, Some(self.ti.clone()));
                    }
                    Ok(())
                };

                // let task = listen_ser((*s).clone(), defaultc.clone(), Some(self.ti.clone()), rx);
                tasks.push((t1, t2));
                shutdown_tx_vec.push(tx);
            });
            debug!("engine will run with {} listens", tasks.len());

            *running = Some(shutdown_tx_vec);
            return Ok(tasks);
        }
    */
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

pub struct FixedOutSelector<'a, T>
where
    T: Iterator<Item = &'a MapperBox> + Clone + Send,
{
    pub mappers: T,
}

impl<'a, T> ruci::relay::conn::OutSelector<'a, T> for FixedOutSelector<'a, T>
where
    T: Iterator<Item = &'a MapperBox> + Clone + Send + Sync,
{
    fn select(&self, _params: Vec<Option<AnyData>>) -> T {
        self.mappers.clone()
    }
}
