#[cfg(any(feature = "api_client", feature = "api_server"))]
mod api;

#[cfg(feature = "utils")]
mod utils;

mod mode;

use std::env::{self, set_var};

use clap::{Parser, Subcommand, ValueEnum};
use tracing::info;

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
    log_level: Option<tracing::Level>,

    /// specify the log file prefix name.
    ///
    /// if empty string is given, no log file will be generated;
    ///
    /// if the flag is not given, log file will be generated with default name
    #[arg(long)]
    log_file: Option<String>,

    /// specify the directory where log files would be in
    ///
    /// if empty string is given, log file will be generated in default folder
    ///
    /// if the flag is not given, log file will be generated in default folder
    #[arg(long)]
    log_dir: Option<String>,

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

    let _g = log_setup(args.clone());

    match args.sub_cmds {
        None => {
            #[cfg(feature = "api_server")]
            {
                let api_server_args = args.api_server.clone();
                let mut started = false;
                for arg in api_server_args {
                    let x = api::server::deal_args(arg, &args).await;
                    if let Some(opts) = x {
                        started = true;
                        start_engine(args.clone(), args.config.clone(), Some(opts)).await?;
                    }
                }
                if !started {
                    start_engine(args.clone(), args.config, None).await?;
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
                    tracing::warn!("{:?}", r)
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

fn log_setup(args: Args) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    println!("ruci-cmd");
    let c_dir = std::env::current_dir().expect("has current directory");
    println!("working dir: {:?}", c_dir);

    println!("Mode: {:?}", args.mode);
    println!("Config: {}", args.config);
    println!("LogLevel(flag): {:?}", args.log_level);

    const RL: &str = "RUST_LOG";

    let mut not_given_flag = false;
    let mut not_given_env = false;

    let given_level = if let Some(l) = args.log_level {
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

    use tracing_appender::{non_blocking, rolling};
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let console_layer = fmt::layer().with_writer(std::io::stderr);

    let logger = tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(console_layer);

    let mut no_file = false;
    let mut file_name = String::from("ruci-cmd.log");

    if let Some(fname) = args.log_file {
        if fname.is_empty() {
            no_file = true;
            println!("Empty log-file name specified, no log file would be generated.")
        } else {
            file_name = fname;
        }
    }

    let guard = if !no_file {
        let file_appender = rolling::daily(args.log_dir.unwrap_or(String::from("logs")), file_name);
        let (non_blocking_appender, guard) = non_blocking(file_appender);
        let file_layer = fmt::layer()
            .json()
            .with_ansi(false)
            .with_writer(non_blocking_appender);
        logger.with(file_layer).init();
        Some(guard)
    } else {
        logger.init();
        None
    };

    println!(
        "Log Level(flag/env): {:?}",
        std::env::var(RL).map_or_else(|_| String::new(), |v| v)
    );

    info!(
        ruci_cmd = env!("CARGO_PKG_VERSION"),
        rucimp = rucimp::VERSION
    );
    if no_file {
        info!("Empty log-file name specified, no log file would be generated.")
    }

    guard
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
