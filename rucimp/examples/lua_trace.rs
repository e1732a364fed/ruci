/*!
在 working dir 或 working dir /resource 或 ../resource/ 文件夹查找 local.lua 或
 用户提供的参数作为配置文件 读取它并以 chain 模式运行. 新连接写入 new_conn.log 文件
*/

use std::env;

use chrono::{DateTime, Utc};
use ruci::relay::*;
use rucimp::{modes::chain::engine::Engine, utils::*, DEFAULT_LUA_CONFIG_FILE_NAME};
use tokio::{
    fs::{File, OpenOptions},
    io::AsyncWriteExt,
};
use tracing::warn;
mod shared;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    shared::print_env_version_and_init_log("example: chain_trace");

    let default_fn = DEFAULT_LUA_CONFIG_FILE_NAME.to_string();

    let args: Vec<String> = env::args().collect();

    let arg_f = if args.len() > 1 {
        Some(args[1].as_str())
    } else {
        None
    };

    let bs = try_get_file_content(&default_fn, arg_f)?;
    let contents = String::from_utf8_lossy(bs.as_slice()).to_string();

    let mut e = Engine::new();

    e.init_lua(contents)?;

    let conn_info_record_file = OpenOptions::new()
        .append(true)
        .create(true)
        .open("new_conn.log")
        .await?;

    let mut fr = FileRecorder {
        f: conn_info_record_file,
        failed: false,
    };

    let (nci_tx, mut nci_rx) = tokio::sync::mpsc::channel(100);

    e.new_conn_recorder = Some(nci_tx);

    #[cfg(feature = "trace")]
    {
        let (ub_tx, mut ub_rx) = tokio::sync::mpsc::channel::<(ruci::net::CID, u64)>(4096);

        let (db_tx, mut db_rx) = tokio::sync::mpsc::channel::<(ruci::net::CID, u64)>(4096);

        e.conn_info_updater = Some((ub_tx, db_tx));

        tokio::spawn(async move {
            loop {
                let x = db_rx.recv().await;
                match x {
                    Some(nc) => {
                        println!("db: {} {}", nc.0, nc.1)
                    }
                    None => break,
                }
            }
        });

        tokio::spawn(async move {
            loop {
                let o = ub_rx.recv().await;
                match o {
                    Some(nc) => {
                        println!("ub: {} {}", nc.0, nc.1)
                    }
                    None => break,
                }
            }
        });
    }

    let mut js = e.run().await?;

    tokio::spawn(async move {
        loop {
            let o = nci_rx.recv().await;
            match o {
                Some(nc) => {
                    if !fr.record(nc).await {
                        break;
                    }
                }
                None => break,
            }
        }
    });

    wait_close_sig().await?;

    e.stop().await;

    js.shutdown().await;

    Ok(())
}

struct FileRecorder {
    f: File,
    failed: bool,
}

impl FileRecorder {
    async fn record(&mut self, state: NewConnInfo) -> bool {
        if self.failed {
            return false;
        }
        let now: DateTime<Utc> = Utc::now();
        let r = self
            .f
            .write(format!("{} {}\n", now, state).as_bytes())
            .await;
        match r {
            Ok(_) => {}
            Err(e) => {
                warn!("conn info write to file failed: {}", e);
                self.failed = true;
            }
        }
        true
    }
}
