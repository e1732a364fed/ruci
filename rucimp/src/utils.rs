use std::{fs, path::PathBuf, process::Command};

use anyhow::{anyhow, bail};
use tokio::signal;
use tracing::{debug, info};

use crate::COMMON_DIRS;

/// try current folder and ruci_config, resource, ../resource folder
///
/// try the default_file given or the first cmd argument
///
/// and will set current dir to the directory
pub fn try_get_file_content(default_file: &str, arg_file: Option<&str>) -> anyhow::Result<String> {
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

pub fn run_command_list(list: Vec<&str>) -> anyhow::Result<()> {
    for cmd in list {
        let mut strs: Vec<_> = cmd.split(' ').collect();
        if strs.is_empty() {
            bail!("got empty command");
        }
        let args = strs.split_off(1);

        debug!(cmd = strs[0], args = ?args, "running command",);

        Command::new(strs[0]).args(args).spawn()?;
    }
    Ok(())
}
