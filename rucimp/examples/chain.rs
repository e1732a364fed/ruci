/*!
* 在 working dir 或 working dir /resource 或 ../resource/ 文件夹查找 local.lua 或
 用户提供的参数作为配置文件 读取它并以 chain 模式运行。
*/

use std::{
    env::{self},
    time::Duration,
};

use log::{debug, info, warn};
use rucimp::{
    chain::{config::lua, engine::Engine},
    example_common::{print_env_version, try_get_filecontent},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    print_env_version("chain");

    let args: Vec<String> = env::args().collect();

    let default_fn = "local.lua".to_string();

    let contents = try_get_filecontent(&default_fn);

    let mut se = Engine::default();
    let sc = lua::load(&contents).expect("has valid lua codes in the file content");

    se.init(sc);

    let se = Box::new(se);

    let r = se.block_run().await;

    warn!("chain engine run returns: {:?}", r);

    if args.len() > 1 && args[1] == "-s" {
        info!("will sleep because of -s");

        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            debug!("sleeped 60s");
        }
    }

    Ok(())
}
