use futures::Future;
use log::debug;
use parking_lot::Mutex;
use ruci::{map::*, net::TransmissionInfo};
use std::{io, sync::Arc};
use tokio::{
    sync::oneshot::{self, Sender},
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
    ) -> std::io::Result<Vec<impl Future<Output = Result<(), std::io::Error>>>> {
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

        //let defaultc = self.default_c.clone().unwrap();

        //todo: 因为没实现路由功能，所以现在只能用一个 client, 即 default client
        // 路由后，要传递给 listen_ser 一个路由表

        let mut tasks = Vec::new();
        let mut shutdown_tx_vec = Vec::new();

        self.servers.iter().for_each(|s| {
            let (tx, rx) = oneshot::channel();

            // let task = listen_ser((*s).clone(), defaultc.clone(), Some(self.ti.clone()), rx);
            // tasks.push(task);
            shutdown_tx_vec.push(tx);
        });
        debug!("engine will run with {} listens", tasks.len());

        *running = Some(shutdown_tx_vec);
        return Ok(tasks);
    }
     */
}
