#[cfg(any(feature = "api_client", feature = "api_server"))]
mod api;

#[cfg(feature = "utils")]
mod utils;

mod mode;

use std::{
    env::{self, set_var},
    sync::Arc,
};

use anyhow::Ok;
use clap::{Parser, Subcommand, ValueEnum};
use log::{info, log_enabled, warn, Level};
use ruci::net::GlobalTrafficRecorder;

#[derive(Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum Mode {
    /// Chain mode, which uses lua file
    #[default]
    C,

    /// Suit mode, which uses toml file
    S,
}

/// ruci command line parameters:
#[derive(Parser, Clone)]
#[command(author = "e")]
#[command(version, about, long_about = None)]
struct Args {
    /// choose the rucimp core mode
    #[arg(short, long, value_enum, default_value_t = Mode::C )]
    mode: Mode,

    /// basic config file
    #[arg(short, long, value_name = "FILE", default_value = "local.lua")]
    config: String,

    #[arg(short, long)]
    log_level: Option<log::Level>,

    #[cfg(feature = "api_server")]
    #[arg(short, long, value_enum)]
    api_server: Vec<api::server::Command>,

    /// default is "127.0.0.1:40681"
    #[cfg(feature = "api_server")]
    #[arg(long)]
    api_addr: Option<String>,

    /// default is "0.0.0.0:18143"
    #[cfg(feature = "api_server")]
    #[arg(long)]
    file_server_addr: Option<String>,

    #[command(subcommand)]
    sub_cmds: Option<SubCommands>,
}

#[derive(Subcommand, Clone)]
enum SubCommands {
    /// api client
    #[cfg(feature = "api_client")]
    ApiClient {
        #[command(subcommand)]
        command: Option<api::client::Commands>,
    },

    /// utils
    #[cfg(feature = "utils")]
    Utils {
        #[command(subcommand)]
        command: Option<utils::Commands>,
    },

    /// configure system route table
    Route,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    print_env_version();
    let args = Args::parse();

    println!("Mode: {:?}", args.mode);
    println!("Path: {}", args.config);
    println!("LogLevel: {:?}", args.log_level);

    match args.sub_cmds {
        None => {
            #[cfg(feature = "api_server")]
            {
                let api_server_args = args.api_server.clone();
                for arg in api_server_args {
                    let x = api::server::deal_args(arg, &args).await;
                    if let Some(opts) = x {
                        start_engine(args.mode.clone(), args.config.clone(), Some(opts)).await?;
                    }
                }
            }
            #[cfg(not(feature = "api_server"))]
            start_engine(args.mode, args.config).await?;
        }

        Some(cs) => match cs {
            #[cfg(feature = "api_client")]
            SubCommands::ApiClient { command } => {
                let r = api::client::deal_cmds(command).await;
                if r.is_err() {
                    warn!("{:?}", r)
                }
            }

            #[cfg(feature = "utils")]
            SubCommands::Utils { command } => {
                utils::deal_cmds(command).await?;
            }
            SubCommands::Route => todo!(),
        },
    }
    Ok(())
}

pub fn print_env_version() {
    println!("ruci-cmd\n");
    let cdir = std::env::current_dir().expect("has current directory");
    println!("working dir: {:?} \n", cdir);

    const RL: &str = "RUST_LOG";
    let l = env::var(RL).unwrap_or("info".to_string());

    if l == "warn" {
        println!("Set env var RUST_LOG to info or debug to see more log.\n powershell like so: $env:RUST_LOG=\"info\";ruci-cmd \n shell like so: RUST_LOG=info ./ruci-cmd")
    }

    set_var(RL, l);
    let _ = env_logger::try_init();

    println!(
        "Log Level(env): {:?}",
        std::env::var(RL).map_or(String::new(), |v| v)
    );

    if !log_enabled!(Level::Info) {
        return;
    }
    info!(
        "version: ruci-cmd:{}, rucimp_{}",
        env!("CARGO_PKG_VERSION"),
        rucimp::VERSION,
    )
}

/// blocking
async fn start_engine(
    m: Mode,
    f: String,
    #[cfg(feature = "api_server")] opts: Option<(
        api::server::Server,
        tokio::sync::mpsc::Receiver<()>,
        Arc<GlobalTrafficRecorder>,
    )>,
) -> anyhow::Result<()> {
    match m {
        Mode::C => {
            mode::chain::run(&f, opts).await?;
        }
        Mode::S => todo!(),
    }
    Ok(())
}
