/*!
* 在 working dir 或 working dir /resource 或 ../resource/ 文件夹查找 local.lua 或
 用户提供的参数作为配置文件 读取它并以 chain 模式运行。
*/

use async_trait::async_trait;
use log::warn;
use ruci::relay::{ConnInfo, InfoRecorder};
use rucimp::{
    example_common::*,
    modes::chain::{config::lua, engine::Engine},
};
use tokio::{fs::File, io::AsyncWriteExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    print_env_version("chain");

    let default_fn = "local.lua".to_string();

    let contents = try_get_filecontent(&default_fn)?;

    let mut se = Engine::default();
    let sc = lua::load(&contents).expect("has valid lua codes in the file content");

    se.init(sc);

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
impl InfoRecorder for FileRecorder {
    async fn record(&mut self, state: ConnInfo) {
        if self.failed {
            return;
        }
        let r = self.f.write(format!("{:?}", state).as_bytes()).await;
        match r {
            Ok(_) => {}
            Err(e) => {
                warn!("conn info write to file failed: {}", e);
                self.failed = true;
            }
        }
    }
}
