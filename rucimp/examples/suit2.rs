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
use rucimp::{
    suit::config::adapter::{
        load_in_mappers_by_str_and_ldconfig, load_out_mappers_by_str_and_ldconfig,
    },
    suit::engine2::SuitEngine,
};

/// 使用 config.suit.toml, resource/config.suit.toml, 或 用户提供的参数作为配置文件
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("rucimp~ suit2\n");
    println!("working dir: {:?} \n", std::env::current_dir().unwrap());

    const RL: &str = "RUST_LOG";
    let l = env::var(RL).unwrap_or("info".to_string());

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

    let args: Vec<String> = env::args().collect();

    let default_file = "config.suit.toml".to_string();

    let filename = if args.len() > 1 {
        &args[1]
    } else {
        &default_file
    };

    let mut r_contents = fs::read_to_string(filename);
    if r_contents.is_err() {
        r_contents = fs::read_to_string("resource/config.suit.toml");
    }

    let contents = r_contents.expect("no config.toml");

    println!("{}", contents);
    let mut se = SuitEngine::new(
        load_in_mappers_by_str_and_ldconfig,
        load_out_mappers_by_str_and_ldconfig,
    );

    se.load_config_from_str(&contents);
    let r = se.block_run().await;

    warn!("r {:?}", r);

    Ok(())
}
