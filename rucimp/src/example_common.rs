use std::{
    env::{self, set_var},
    fs,
    path::PathBuf,
};

use log::{debug, info, log_enabled, Level};
use tokio::signal::{self};

pub fn print_env_version(name: &str) {
    println!("rucimp~ {}\n", name);
    let cdir = std::env::current_dir().expect("has current directory");
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
        info!("version: rucimp_{}", crate::VERSION,)
    }
}

/// try current folder and ruci_config, resource, ../resource folder
///
/// try the default_file given or the first cmd argument
pub fn try_get_filecontent(default_file: &str) -> String {
    let cdir = std::env::current_dir().expect("has current directory");
    let args: Vec<String> = env::args().collect();

    let filename = if args.len() > 1 && args[1] != "-s" {
        &args[1]
    } else {
        default_file
    };

    let mut r_contents = fs::read_to_string(PathBuf::from(filename));
    if r_contents.is_err() {
        debug!("try ruci_config folder");
        let mut cd = cdir.clone();
        cd.push("ruci_config");

        if cd.exists() {
            std::env::set_current_dir(cd).expect("set_current_dir ok");
            r_contents = fs::read_to_string(filename);
        }
    }

    if r_contents.is_err() {
        debug!("try resource folder");
        let mut cd = cdir.clone();
        cd.push("resource");

        if cd.exists() {
            std::env::set_current_dir(cd).expect("set_current_dir ok");
            r_contents = fs::read_to_string(filename);
        }
    }
    if r_contents.is_err() {
        debug!("try ../resource folder");

        let mut cd = cdir.clone();
        cd.push("../resource");

        if cd.exists() {
            std::env::set_current_dir(cd).expect("set_current_dir ok");
            r_contents = fs::read_to_string(filename);
        }
    }
    r_contents.expect(&format!("no such file: {}", filename))
}

pub async fn wait_close_sig() -> anyhow::Result<()> {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(unix)]
    let terminate2 = async {
        signal::unix::signal(signal::unix::SignalKind::interrupt())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    #[cfg(not(unix))]
    let terminate2 = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("got ctrl_c"),
        _ = terminate => info!("got terminate"),
        _ = terminate2 => info!("got interrupt"),
    }

    info!("signal received, starting graceful shutdown...");

    Ok(())
}
