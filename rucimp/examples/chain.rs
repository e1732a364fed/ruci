/*!
 * rucimp 提供数个示例可执行文件, 若要全功能, 用 rucimple
 *
 * 在working dir 或 working dir /resource 文件夹查找 config.toml 文件, 读取它并以suit模式运行。
 */

use std::{
    env::{self, set_var},
    fs,
};

use log::{info, log_enabled, warn, Level};
use rucimp::chain::{config::lua, engine::StaticEngine};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("rucimp~ chain\n");
    println!("working dir: {:?} \n", std::env::current_dir().unwrap());

    const RL: &str = "RUST_LOG";
    let l = env::var(RL).unwrap_or("warn".to_string());

    if l == "warn" {
        println!("Set env var RUST_LOG to info or debug to see more log.\n powershell like so: $env:RUST_LOG=\"info\";rucimp \n shell like so: RUST_LOG=info ./rucimp")
    }

    set_var(RL, l);
    env_logger::init();

    println!(
        "Log Level(env): {:?}",
        std::env::var(RL).map_or(String::new(), |v| v)
    );

    if log_enabled!(Level::Info) {
        info!("versions: ruci_{}_mp_{}", ruci::VERSION, rucimp::VERSION,)
    }

    let mut r_contents = fs::read_to_string("config.chain.lua");
    if r_contents.is_err() {
        r_contents = fs::read_to_string("resource/config.chain.lua");
    }

    let contents = r_contents.expect("no config.chain.lua");

    // println!("{}", contents);
    let mut se = StaticEngine::default();
    let sc = lua::load(&contents).unwrap();
    se.init(sc);

    let se = Box::new(se);
    let se: &'static StaticEngine = Box::leak(se);

    let r = se.block_run().await;

    warn!("r {:?}", r);

    Ok(())
}
