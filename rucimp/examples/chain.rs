/*!
* 在 working dir 或 working dir /resource 或 ../resource/ 文件夹查找 local.lua 或
 用户提供的参数作为配置文件 读取它并以 chain 模式运行。
*/

use std::{
    env::{self, set_var},
    fs,
    path::PathBuf,
    time::Duration,
};

use log::{debug, info, log_enabled, warn, Level};
use rucimp::chain::{config::lua, engine::StaticEngine};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("rucimp~ chain\n");
    let cdir = std::env::current_dir().unwrap();
    println!("working dir: {:?} \n", cdir);

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
        info!("version: rucimp_{}", rucimp::VERSION,)
    }

    let args: Vec<String> = env::args().collect();

    let default_file = "local.lua".to_string();

    let filename = if args.len() > 1 && args[1] != "-s" {
        &args[1]
    } else {
        &default_file
    };

    let mut r_contents = fs::read_to_string(PathBuf::from(filename));
    if r_contents.is_err() {
        debug!("try resource folder");
        let mut cd = cdir.clone();
        cd.push("resource");

        if cd.exists() {
            std::env::set_current_dir(cd)?;
            r_contents = fs::read_to_string(filename);
        }
    }
    if r_contents.is_err() {
        debug!("try ../resource folder");

        let mut cd = cdir.clone();
        cd.push("../resource");

        if cd.exists() {
            std::env::set_current_dir(cd)?;
            r_contents = fs::read_to_string(filename);
        }
    }

    let contents = r_contents.expect(&("no ".to_owned() + &filename));

    let mut se = StaticEngine::default();
    let sc = lua::load(&contents).unwrap();

    //println!("{:?}", sc);

    se.init(sc);

    let se = Box::new(se);
    let se: &'static mut StaticEngine = Box::leak(se);

    let r = se.block_run().await;

    warn!("r {:?}", r);

    if args.len() > 1 && args[1] == "-s" {
        info!("will sleep because of -s");

        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            debug!("sleeped 60s");
        }
    }

    Ok(())
}
