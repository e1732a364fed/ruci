use log::{info, log_enabled, Level};
use std::env::{self, set_var};

pub fn print_env_version(name: &str) {
    println!("rucimp~ {}\n", name);
    let cdir = std::env::current_dir().expect("has current directory");
    println!("working dir: {:?} \n", cdir);

    const RL: &str = "RUST_LOG";
    let l = env::var(RL).unwrap_or_else(|_| "info".to_string());

    if l == "warn" {
        println!("Set env var RUST_LOG to info or debug to see more log.\n powershell like so: $env:RUST_LOG=\"info\";rucimp \n shell like so: RUST_LOG=info ./rucimp")
    }

    set_var(RL, l);
    env_logger::init();

    println!(
        "Log Level(env): {:?}",
        std::env::var(RL).map_or_else(|_| String::new(), |v| v)
    );

    if log_enabled!(Level::Info) {
        info!("version: rucimp_{}", rucimp::VERSION,)
    }
}
