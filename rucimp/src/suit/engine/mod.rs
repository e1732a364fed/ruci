pub mod relay;

use std::{io, sync::Arc};

use crate::suit::*;
use config::LDConfig;
use futures::{future::select_all, Future};
use log::{debug, info};
use parking_lot::Mutex;
use ruci::{map::*, net::TransmissionInfo};

use tokio::{
    sync::oneshot::{self, Sender},
    task,
};

use super::config;

pub struct SuitEngine<FInadder, FOutadder>
where
    FInadder: Fn(&str, LDConfig) -> Option<MapperBox> + 'static,
    FOutadder: Fn(&str, LDConfig) -> Option<MapperBox> + 'static,
{
    pub running: Arc<Mutex<Option<Vec<Sender<()>>>>>, //这里约定, 所有对 engine的热更新都要先访问running的锁
    pub ti: Arc<TransmissionInfo>,

    servers: Vec<&'static dyn Suit>,
    clients: Vec<&'static dyn Suit>,
    default_c: Option<&'static dyn Suit>,

    load_inmappers_func: FInadder,
    load_outmappers_func: FOutadder,
}

impl<LI, LO> SuitEngine<LI, LO>
where
    LI: Fn(&str, LDConfig) -> Option<MapperBox> + 'static,
    LO: Fn(&str, LDConfig) -> Option<MapperBox> + 'static,
{
    pub fn new(load_inmapper_func: LI, load_outmapper_func: LO) -> Self {
        SuitEngine {
            ti: Arc::new(TransmissionInfo::default()),
            servers: Vec::new(),
            clients: Vec::new(),
            default_c: None,
            running: Arc::new(Mutex::new(None)),
            load_inmappers_func: load_inmapper_func,
            load_outmappers_func: load_outmapper_func,
        }
    }

    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// convert and calls load_config
    pub fn load_config_from_str(&mut self, s: &str) {
        //todo: 修改 suit::config::Config 的结构后要改这里
        let c: crate::suit::config::Config = crate::suit::config::Config::from_toml(s);
        self.load_config(c);
    }

    pub fn load_config(&mut self, c: crate::suit::config::Config) {
        self.clients = c
            .dial
            .iter()
            .map(|lc| {
                let mut c = SuitStruct::from(lc.clone());
                c.set_behavior(ProxyBehavior::ENCODE);
                c.generate_upper_mappers();
                let r_proxy_outadder = (self.load_outmappers_func)(c.protocol(), c.config.clone());
                if let Some(proxy_outadder) = r_proxy_outadder {
                    c.push_mapper(Arc::new(proxy_outadder));
                }

                let outc: &'static dyn Suit = Box::leak(Box::new(c));
                if let None = self.default_c {
                    self.default_c = Some(outc);
                }
                outc
            })
            .collect();

        if self.clients.is_empty() {
            let d = Box::leak(Box::new(direct_suit()));
            self.default_c = Some(d);
            self.clients.push(d);
        }

        self.servers = c
            .listen
            .iter()
            .map(|lc| {
                let mut s = SuitStruct::from(lc.clone());
                s.set_behavior(ProxyBehavior::DECODE);

                s.generate_upper_mappers();
                let r_proxy_inadder = (self.load_inmappers_func)(s.protocol(), s.config.clone());
                if let Some(proxy_inadder) = r_proxy_inadder {
                    s.push_mapper(Arc::new(proxy_inadder));
                }

                let ins: &'static dyn Suit = Box::leak(Box::new(s));
                ins
            })
            .collect();
    }

    /// non-blocking, return true if run succeed;
    /// calls start_with_tasks
    pub async fn run(&self) -> io::Result<()> {
        self.start_with_tasks().await.map(|tasks| {
            for task in tasks {
                task::spawn(task);
            }
        })
    }

    /// blocking, return only if all servers stoped listening.
    /// calls start_with_tasks
    ///
    /// 该方法不能用 block_on 调用, 只能用 await
    pub async fn block_run(&self) -> io::Result<()> {
        /*
        let rtasks = self.start_with_tasks().await;
        (join_all(rtasks?).await);
        */
        use futures_lite::future::FutureExt;

        let rtasks = self.start_with_tasks().await?;
        let (result, _, _remaining_tasks) =
            select_all(rtasks.into_iter().map(|task| task.boxed())).await;

        result
    }

    /// called by block_run and run. Must call after calling load_config
    pub async fn start_with_tasks(
        &self,
    ) -> std::io::Result<Vec<impl Future<Output = Result<(), std::io::Error>>>> {
        let mut running = self.running.lock();
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

        let defaultc = self.default_c.clone().expect("has default_c");

        //todo: 因为没实现路由功能, 所以现在只能用一个 client, 即 default client
        // 路由后, 要传递给 listen_ser 一个路由表

        let mut tasks = Vec::new();
        let mut shutdown_tx_vec = Vec::new();

        self.servers.iter().for_each(|s| {
            let (tx, rx) = oneshot::channel();

            let task = relay::listen_ser(*s, defaultc, Some(self.ti.clone()), rx);
            tasks.push(task);
            shutdown_tx_vec.push(tx);
        });
        debug!("engine will run with {} listens", tasks.len());

        *running = Some(shutdown_tx_vec);
        Ok(tasks)
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

        let ss = self.servers.as_slice();
        for s in ss {
            s.stop();
        }
        info!("stopped");
    }
}
