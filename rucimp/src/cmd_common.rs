use std::{
    env::{self, set_var},
    fs,
    path::PathBuf,
};

use anyhow::anyhow;
use log::{debug, info, log_enabled, Level};
use tokio::signal;

use crate::COMMON_DIRS;

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
///
/// and will set current dir to the directory
pub fn try_get_filecontent(default_file: &str, arg_file: Option<&str>) -> anyhow::Result<String> {
    let filename = match arg_file.as_ref() {
        Some(a) => a,
        None => default_file,
    };

    let mut last_e: Option<std::io::Error> = None;
    for dir in &COMMON_DIRS {
        let s = String::from(*dir) + filename;

        let r = fs::read_to_string(PathBuf::from(s));
        match r {
            Ok(r) => {
                let mut cd = std::env::current_dir().expect("has current directory");

                cd.push(dir);

                if cd.exists() {
                    std::env::set_current_dir(cd).expect("set_current_dir ok");
                    debug!("set current dir to {:?}", std::env::current_dir());
                }

                return Ok(r);
            }
            Err(e) => last_e = Some(e),
        }
    }

    match last_e {
        Some(e) => Err(e.into()),
        None => Err(anyhow!("open {filename} failed and no result err")),
    }
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

pub async fn wait_close_sig_with_closer(
    mut c: tokio::sync::mpsc::Receiver<()>,
) -> anyhow::Result<()> {
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
        _ = c.recv() => info!("GOT user close"),
        _ = ctrl_c => info!("got ctrl_c"),
        _ = terminate => info!("got terminate"),
        _ = terminate2 => info!("got interrupt"),
    }

    info!("signal received, starting graceful shutdown...");

    Ok(())
}
