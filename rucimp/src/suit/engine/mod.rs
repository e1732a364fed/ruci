use std::{io, sync::Arc};

use crate::suit::*;
use config::LDConfig;
use futures::Future;
use log::{debug, info};
use parking_lot::Mutex;
use ruci::{
    map::*,
    net::{Stream, TrafficRecorder},
    relay::{self, route::*},
};

use tokio::{
    sync::oneshot::{self, Sender},
    task,
};

use super::config;

pub struct SuitEngine {
    pub running: Arc<Mutex<Option<Vec<Sender<()>>>>>, //这里约定, 所有对 engine的热更新都要先访问running的锁
    pub ti: Arc<TrafficRecorder>,

    servers: Vec<Arc<Box<dyn Suit>>>,
    clients: Vec<Arc<Box<dyn Suit>>>,
    default_c: Option<Arc<Box<dyn Suit>>>,
}

impl SuitEngine {
    pub fn new() -> Self {
        SuitEngine {
            ti: Arc::new(TrafficRecorder::default()),
            servers: Vec::new(),
            clients: Vec::new(),
            default_c: None,
            running: Arc::new(Mutex::new(None)),
        }
    }

    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// convert and calls load_config
    pub fn load_config_from_str<FInadder, FOutadder>(
        &mut self,
        s: &str,
        load_inmappers_func: FInadder,
        load_outmappers_func: FOutadder,
    ) where
        FInadder: Fn(&str, LDConfig) -> Option<MapperBox>,
        FOutadder: Fn(&str, LDConfig) -> Option<MapperBox>,
    {
        //todo: 修改 suit::config::Config 的结构后要改这里
        let c: crate::suit::config::Config = crate::suit::config::Config::from_toml(s);
        self.load_config(c, load_inmappers_func, load_outmappers_func);
    }

    pub fn load_config<FInadder, FOutadder>(
        &mut self,
        c: crate::suit::config::Config,
        load_inmappers_func: FInadder,
        load_outmappers_func: FOutadder,
    ) where
        FInadder: Fn(&str, LDConfig) -> Option<MapperBox>,
        FOutadder: Fn(&str, LDConfig) -> Option<MapperBox>,
    {
        self.clients = c
            .dial
            .iter()
            .map(|lc| {
                let mut s = SuitStruct::from(lc.clone());
                s.set_behavior(ProxyBehavior::ENCODE);
                s.generate_upper_mappers();
                let r_proxy_outadder = load_outmappers_func(s.protocol(), s.config.clone());
                if let Some(proxy_outadder) = r_proxy_outadder {
                    s.push_mapper(Arc::new(proxy_outadder));
                }
                let x: Box<dyn Suit> = Box::new(s);
                Arc::new(x)
            })
            .collect();

        if self.clients.is_empty() {
            let d: Box<dyn Suit> = Box::new(direct_suit());

            self.clients.push(Arc::new(d));
        }

        let d = self.clients.first().expect("has a client");
        self.default_c = Some(d.clone());

        let x: Vec<_> = c
            .listen
            .iter()
            .map(|lc| {
                let mut s = SuitStruct::from(lc.clone());
                s.set_behavior(ProxyBehavior::DECODE);

                s.generate_upper_mappers();
                let r_proxy_inadder = load_inmappers_func(s.protocol(), s.config.clone());
                if let Some(proxy_inadder) = r_proxy_inadder {
                    s.push_mapper(Arc::new(proxy_inadder));
                }
                let x: Box<dyn Suit> = Box::new(s);
                Arc::new(x)
            })
            .collect();

        self.servers = x;
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
    pub async fn block_run(&self) -> io::Result<Vec<io::Result<()>>> {
        let rtasks = self.start_with_tasks().await?;
        Ok(futures::future::join_all(rtasks).await)

        // use futures_lite::future::FutureExt;

        // let rtasks = self.start_with_tasks().await?;
        // let (result, _, _remaining_tasks) =
        //     select_all(rtasks.into_iter().map(|task| task.boxed())).await;
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

        //todo: 因为没实现路由功能, 所以现在只能用一个 client, 即 default client
        // 路由后, 要传递给 listen_ser 一个路由表

        let mut tasks = Vec::new();
        let mut shutdown_tx_vec = Vec::new();

        self.servers.clone().into_iter().for_each(|s| {
            let (tx, rx) = oneshot::channel();

            let task = listen_ser(
                s,
                self.default_c.clone().expect("has default_c"),
                Some(self.ti.clone()),
                rx,
            );
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

pub async fn listen_ser(
    ins: Arc<Box<dyn Suit>>,
    outc: Arc<Box<dyn Suit>>,
    oti: Option<Arc<net::TrafficRecorder>>,
    shutdown_rx: oneshot::Receiver<()>,
) -> io::Result<()> {
    let n = ins.network();
    match n {
        "tcp" => {
            if outc.network() != "tcp" {
                panic!(
                    "not implemented for dialing network other than tcp: {}",
                    outc.network()
                )
            }
            listen_tcp(ins, outc, oti, shutdown_rx).await
        }
        _ => Err(io::Error::other(format!(
            "such network not supported: {}",
            n
        ))),
    }
}

/// blocking loop listen ins tcp。calls handle_conn_clonable inside the loop.
async fn listen_tcp(
    ins: Arc<Box<dyn Suit>>,
    outc: Arc<Box<dyn Suit>>,
    oti: Option<Arc<net::TrafficRecorder>>,
    shutdown_rx: oneshot::Receiver<()>,
) -> io::Result<()> {
    let laddr = ins.addr_str().to_string();
    let wn = ins.whole_name().to_string();
    info!("start listen {}, {}", laddr, wn);

    let listener = TcpListener::bind(laddr.clone()).await?;

    let clone_oti = move || oti.clone();

    let iter = outc.get_mappers_vec().into_iter();
    let ib = Box::new(iter);
    let selector: Box<dyn OutSelector> = Box::new(FixedOutSelector { default: ib });
    let selector = Arc::new(selector);

    tokio::select! {
        r = async {
            loop {
                let (tcpstream, raddr) = listener.accept().await?;

                let ti = clone_oti();
                if log_enabled!(Debug) {
                    debug!("new tcp in, laddr:{}, raddr: {:?}", laddr, raddr);
                }

                let iter = ins.get_mappers_vec().into_iter();
                let ib = Box::new(iter);

                let slt = selector.clone();
                tokio::spawn(  relay::handle_in_stream(
                        Stream::c(Box::new(tcpstream)),
                        ib,
                        slt,
                        ti,
                    )
                );
            }

        } => {
            r

        }
        _ = shutdown_rx => {
            info!("terminating accept loop, {} ",wn );
            Ok(())
        }
    }
}
