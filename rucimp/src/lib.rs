/*!
定义一个 SuitEngine struct，用于同时执行多个代理

子suit定义了 套装suit, 内部包含了关键的转发逻辑
*/
pub mod suit;

pub const VERSION: &str = "0.0.0";

use std::{io, sync::Arc};

use futures::{executor::block_on, future::select_all, Future};
use log::debug;
use ruci::{map::*, net::TransmissionInfo};
use suit::config::LDConfig;
use suit::*;

/// 将所有在本包中实现的 in_adder 从 名称映射到 InAdderBox.
///
/// 可作为 SuitEngine::new 的参数
pub fn load_in_mappers_by_str_and_ldconfig(s: &str, c: LDConfig) -> Option<MapperBox> {
    match s {
        "adder" => {
            let a = ruci::map::math::Adder {
                addnum: c.number_arg.unwrap_or(1) as i8,
            };
            Some(Box::new(a))
        }
        "counter" => {
            let a = ruci::map::counter::Counter;
            Some(Box::new(a))
        }
        "socks5" => {
            let a = block_on(socks5::server::Server::new(
                suit::config::adapter::get_socks5_server_option_from_ldconfig(c),
            ));
            Some(Box::new(a))
        }

        _ => None,
    }
}

/// 将所有在本包中实现的 out_adder 从 名称映射到 OutAdderBox.
///
/// 可作为 SuitEngine::new 的参数
pub fn load_out_mappers_by_str_and_ldconfig(s: &str, _: LDConfig) -> Option<MapperBox> {
    match s {
        _ => None,
    }
}

use serde::{Deserialize, Serialize};
use tokio::{sync::Mutex, task};

/// Engine 级别的 Config，比proxy级的 Config 多了一些信息，如api server部分和 engine 部分
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub proxy_config: suit::config::Config,
    //todo: api_server_config, engine_config
}

pub struct SuitEngine<FInadder, FOutadder>
where
    FInadder: Fn(&str, LDConfig) -> Option<MapperBox> + 'static,
    FOutadder: Fn(&str, LDConfig) -> Option<MapperBox> + 'static,
{
    running: Arc<Mutex<u8>>, //这里约定，所有对 engine的热更新都要先访问running的锁

    servers: Vec<Arc<dyn Suit>>,
    clients: Vec<Arc<dyn Suit>>,
    default_c: Option<Arc<dyn Suit>>,

    ti: Arc<TransmissionInfo>,

    load_inmappers_func: FInadder,
    load_outmappers_func: FOutadder,
}

impl<LI, LO> SuitEngine<LI, LO>
where
    LI: Fn(&str, LDConfig) -> Option<MapperBox> + 'static,
    LO: Fn(&str, LDConfig) -> Option<MapperBox> + 'static,
{
    pub fn new(load_inadder_func: LI, load_outadder_func: LO) -> Self {
        SuitEngine {
            ti: Arc::new(TransmissionInfo::default()),
            servers: Vec::new(),
            clients: Vec::new(),
            default_c: None,
            running: Arc::new(Mutex::new(0)),
            load_inmappers_func: load_inadder_func,
            load_outmappers_func: load_outadder_func,
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
        let c: suit::config::Config = suit::config::Config::from_toml(s);
        let c = Config { proxy_config: c };
        self.load_config(c);
    }

    pub fn load_config(&mut self, c: Config) {
        self.clients = c
            .proxy_config
            .dial
            .iter()
            .map(|lc| {
                let mut s = SuitStruct::from(lc.clone());
                s.set_behavior(ProxyBehavior::ENCODE);
                s.generate_upper_mappers();
                let r_proxy_outadder = (self.load_outmappers_func)(s.protocol(), s.config.clone());
                if let Some(proxy_outadder) = r_proxy_outadder {
                    s.push_mapper(proxy_outadder);
                }
                let x: Arc<dyn Suit> = Arc::new(s);
                x
            })
            .collect();

        if self.clients.len() == 0 {
            let d = Arc::new(direct_suit());
            self.clients.push(d);
        }

        self.default_c = Some(self.clients.first().unwrap().clone());

        self.servers = c
            .proxy_config
            .listen
            .iter()
            .map(|lc| {
                let mut s = SuitStruct::from(lc.clone());
                s.set_behavior(ProxyBehavior::DECODE);

                s.generate_upper_mappers();
                let r_proxy_inadder = (self.load_inmappers_func)(s.protocol(), s.config.clone());
                if let Some(proxy_inadder) = r_proxy_inadder {
                    s.push_mapper(proxy_inadder);
                }
                let x: Arc<dyn Suit> = Arc::new(s);
                x
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
    /// 该方法不能用 block_on 调用，只能用 await
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
        let mut running = self.running.lock().await;
        if *running == 1 {
            return Err(io::Error::other("already started!"));
        }
        if self.server_count() == 0 {
            return Err(io::Error::other("no server"));
        }
        if self.client_count() == 0 {
            return Err(io::Error::other("no client"));
        }

        *running = 1;
        let defaultc = self.default_c.clone().unwrap();

        //todo: 因为没实现路由功能，所以现在只能用一个 client, 即 default client
        // 路由后，要传递给 listen_ser 一个路由表

        let mut tasks = Vec::new();
        self.servers.iter().for_each(|s| {
            let task = listen_ser((*s).clone(), defaultc.clone(), Some(self.ti.clone()));
            tasks.push(task);
        });
        debug!("engine will run with {} tasks", tasks.len());
        return Ok(tasks);
    }

    /// 停止所有的 server, 但并不清空配置。意味着可以stop后接着调用 run
    pub async fn stop(&self) {
        let mut running = self.running.lock().await;
        *running = 0;

        let ss = self.servers.as_slice();
        for s in ss {
            s.stop();
        }
    }
}
