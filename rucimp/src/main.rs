/*!
 * rucimp 提供一个简易可执行文件，若要全功能，用 rucimple
 *
 * 查找 config.toml 文件，读取它并以suit模式运行。
 */

use std::{
    env::{self, set_var},
    fs,
};

use log::{info, log_enabled, Level};
use rucimp::{
    suit::config::adapter::{
        load_in_mappers_by_str_and_ldconfig, load_out_mappers_by_str_and_ldconfig,
    },
    suit::engine::SuitEngine,
};
//use rucimp::{load_in_adder_by_str, load_out_adder_by_str, SuitEngine};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("rucimp~ \n");

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

    let contents = fs::read_to_string("config.toml").expect("no config.toml");
    println!("{}", contents);
    let mut se = SuitEngine::new(
        load_in_mappers_by_str_and_ldconfig,
        load_out_mappers_by_str_and_ldconfig,
    );

    se.load_config_from_str(&contents);
    let r = se.block_run().await;

    info!("r {:?}", r);

    Ok(())
}
