#[cfg(any(feature = "api_client", feature = "api_server"))]
mod api;

#[cfg(feature = "utils")]
mod utils;

mod mode;

use std::env::{self, set_var};

use clap::{Parser, Subcommand, ValueEnum};
use log::{info, log_enabled, Level};

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

    /// use infinite dynamic chain that is written in the lua config file (the "infinite"
    /// global variable must exist)
    #[cfg(feature = "lua")]
    #[arg(long)]
    infinite: bool,

    /// enable flux trace (might slow down performance)
    #[cfg(feature = "trace")]
    #[arg(long)]
    trace: bool,

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
    let args = Args::parse();

    println!("Mode: {:?}", args.mode);
    println!("Config: {}", args.config);
    println!("LogLevel: {:?}", args.log_level);

    print_env_version(args.log_level);

    match args.sub_cmds {
        None => {
            #[cfg(feature = "api_server")]
            {
                let api_server_args = args.api_server.clone();
                for arg in api_server_args {
                    let x = api::server::deal_args(arg, &args).await;
                    if let Some(opts) = x {
                        start_engine(args.clone(), args.config.clone(), Some(opts)).await?;
                    }
                }
            }
            #[cfg(not(feature = "api_server"))]
            start_engine(args.clone(), args.config).await?;
        }

        Some(cs) => match cs {
            #[cfg(feature = "api_client")]
            SubCommands::ApiClient { command } => {
                let r = api::client::deal_cmds(command).await;
                if r.is_err() {
                    log::warn!("{:?}", r)
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

pub fn print_env_version(ll: Option<Level>) {
    println!("ruci-cmd\n");
    let cdir = std::env::current_dir().expect("has current directory");
    println!("working dir: {:?} \n", cdir);

    const RL: &str = "RUST_LOG";

    let mut not_given_flag = false;
    let mut not_given_env = false;

    let given_level = if let Some(l) = ll {
        l.as_str()
    } else {
        not_given_flag = true;
        "info"
    };

    let l = env::var(RL).unwrap_or_else(|_| {
        not_given_env = true;
        given_level.to_string()
    });

    if not_given_flag && not_given_env {
        println!("Set env var RUST_LOG to info or debug to see more log.\n powershell like so: $env:RUST_LOG=\"info\";ruci-cmd \n shell like so: RUST_LOG=info ./ruci-cmd\n");

        println!("You can also set -l or --log-level flag, but RUST_LOG has the highest priority\n")
    }

    set_var(RL, l);

    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    println!(
        "Log Level(env): {:?}",
        std::env::var(RL).map_or_else(|_| String::new(), |v| v)
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
    args: Args,
    f: String,
    #[cfg(feature = "api_server")] opts: Option<(
        api::server::Server,
        tokio::sync::mpsc::Receiver<()>,
        std::sync::Arc<ruci::net::GlobalTrafficRecorder>,
    )>,
) -> anyhow::Result<()> {
    match args.mode {
        Mode::C => {
            mode::chain::run(
                &f,
                args,
                #[cfg(feature = "api_server")]
                opts,
            )
            .await?;
        }
        Mode::S => todo!(),
    }
    Ok(())
}
