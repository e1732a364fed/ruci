/*!
* 在 working dir 或 working dir /resource 或 ../resource/ 文件夹查找 local.lua 或
 用户提供的参数作为配置文件 读取它并以 chain 模式运行。
*/

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::warn;
use ruci::relay::record::*;
use rucimp::{
    example_common::*,
    modes::chain::{config::lua, engine::Engine},
};
use tokio::{
    fs::{File, OpenOptions},
    io::AsyncWriteExt,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    print_env_version("chain");

    let default_fn = "local.lua".to_string();

    let contents = try_get_filecontent(&default_fn)?;

    let mut se = Engine::default();
    let sc = lua::load(&contents).expect("has valid lua codes in the file content");

    se.init(sc);

    let conn_info_record_file = OpenOptions::new()
        .append(true)
        .create(true)
        .open("newconn.log")
        .await?;

    let fr = FileRecorder {
        f: conn_info_record_file,
        failed: false,
    };
    se.conn_info_recorder = Some(Arc::new(tokio::sync::Mutex::new(Box::new(fr))));

    let se = Box::new(se);

    se.run().await?;

    wait_close_sig().await?;

    se.stop().await;

    Ok(())
}

struct FileRecorder {
    f: File,
    failed: bool,
}

#[async_trait]
impl NewInfoRecorder for FileRecorder {
    async fn record(&mut self, state: NewConnInfo) {
        if self.failed {
            return;
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
    }
}
