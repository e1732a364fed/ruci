use std::env::{self, set_var};

use tracing::info;

fn init_log() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    let _ = tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init();
}

pub fn print_env_version(name: &str) {
    println!("rucimp~ {}\n", name);
    let c_dir = std::env::current_dir().expect("has current directory");
    println!("working dir: {:?} \n", c_dir);

    const RL: &str = "RUST_LOG";
    let l = env::var(RL).unwrap_or_else(|_| "info".to_string());

    if l == "warn" {
        println!("Set env var RUST_LOG to info or debug to see more log.\n powershell like so: $env:RUST_LOG=\"info\";rucimp \n shell like so: RUST_LOG=info ./rucimp")
    }

    set_var(RL, l);
    init_log();

    println!(
        "Log Level(env): {:?}",
        std::env::var(RL).map_or_else(|_| String::new(), |v| v)
    );

    info!("version: rucimp_{}", rucimp::VERSION,)
}
