/*!
具有综合功能的命令行程序

可选功能 file_server, api_client, api_server, utils

针对 rucimp 核心的 可选功能:
trace, quic, quinn, lua, lua54, use-native-tls, native-tls-vendored, steganography

 */
#[cfg(any(feature = "api_client", feature = "api_server"))]
mod api;

#[cfg(feature = "utils")]
mod utils;

mod mode;

use std::env::{self, set_var};

use clap::{Parser, Subcommand, ValueEnum};
use rucimp::DEFAULT_LUA_CONFIG_FILE_NAME;
use tracing::{debug, info};

#[derive(Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum Mode {
    /// Chain mode, which uses lua/toml file
    #[default]
    C,
}

/// ruci command line parameters:
#[derive(Parser, Clone)]
#[command(author = "e")]
#[command(version, about, long_about = None)]
struct Args {
    /// choose the rucimp core mode
    #[arg(short, long, value_enum, default_value_t = Mode::C )]
    mode: Mode,

    /// Basic config file. Can be of lua or toml format.
    ///
    /// If the given string is a url, then the app will try to download the file first.
    #[arg(short, long, value_name = "FILE", default_value = DEFAULT_LUA_CONFIG_FILE_NAME)]
    config: String,

    /// If this arg is given, and the "config" arg is a url, then the app will try to download
    /// the config file but will not store it in the file system.
    ///
    /// This will cause the app to download the config file every time it runs.
    #[arg(long)]
    in_memory: bool,

    #[arg(short, long)]
    log_level: Option<tracing::Level>,

    /// Specify the log file prefix name.
    ///
    /// if empty string is given, no log file will be generated;
    ///
    /// if the flag is not given, log file will be generated with default name
    #[arg(long)]
    log_file: Option<String>,

    /// Specify the directory where log files would be in
    ///
    /// if empty string is given, log file will be generated in default folder
    ///
    /// if the flag is not given, log file will be generated in default folder
    #[arg(long)]
    log_dir: Option<String>,

    /// Use infinite dynamic chain that is written in the lua config file (the "Infinite"
    /// global variable must exist)
    #[cfg(any(feature = "lua", feature = "lua54"))]
    #[arg(long)]
    infinite: bool,

    /// Enable flux trace (might slow down performance)
    #[cfg(feature = "trace")]
    #[arg(long)]
    trace: bool,

    #[cfg(feature = "api_server")]
    #[arg(short, long, value_enum)]
    api_server: Vec<api::server::Command>,

    /// Default is "127.0.0.1:40681"
    #[cfg(feature = "api_server")]
    #[arg(long)]
    api_addr: Option<String>,

    #[command(subcommand)]
    sub_cmds: Option<SubCommands>,
}

#[derive(Subcommand, Clone)]
enum SubCommands {
    /// Api client
    #[cfg(feature = "api_client")]
    ApiClient {
        #[command(subcommand)]
        command: Option<api::client::Commands>,
    },

    /// Utilities
    #[cfg(feature = "utils")]
    Utils {
        #[command(subcommand)]
        command: Option<utils::Commands>,
    },
    // Configure system route table
    // Route,
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
                    let oa = api::server::deal_args(arg, &args).await;
                    if let Some(opts) = oa {
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
            } // SubCommands::Route => todo!(),
        },
    }
    Ok(())
}

// 注：返回的 gaurd 超出作用域(被回收)后，log 结束
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
        println!("Set env var RUST_LOG to info or debug to see more log.\n powershell like so: $env:RUST_LOG=\"info\";.\\ruci-cmd \n shell like so: RUST_LOG=info ./ruci-cmd\n");

        println!("You can also set -l or --log-level flag, but RUST_LOG has the highest priority\n")
    }

    set_var(RL, l);

    use tracing_appender::{non_blocking, rolling};
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let console_layer = fmt::layer()
        .with_line_number(true)
        .with_writer(std::io::stderr);

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
        std::env::var(RL).unwrap_or_else(|_| String::new())
    );

    #[allow(unused_mut)]
    let mut features_list: Vec<&str> = vec![
        #[cfg(feature = "file_server")]
        "file_server",
        #[cfg(feature = "api_server")]
        "api_server",
        #[cfg(feature = "api_client")]
        "api_client",
        #[cfg(feature = "utils")]
        "utils",
        #[cfg(feature = "trace")]
        "trace",
        #[cfg(feature = "use-native-tls")]
        "native-tls",
        #[cfg(feature = "native-tls-vendored")]
        "native-tls-vendored",
        #[cfg(feature = "lua")]
        "lua",
        #[cfg(feature = "lua54")]
        "lua54",
        #[cfg(feature = "quinn")]
        "quinn",
        // #[cfg(feature = "quic")]
        // "quic",
        #[cfg(feature = "tun")]
        "tun",
        #[cfg(feature = "smoltcp")]
        "smoltcp",
        #[cfg(feature = "steganography")]
        "steganography",
    ];

    info!(
        ruci_cmd = env!("CARGO_PKG_VERSION"),
        rucimp = rucimp::VERSION,
        features = ?features_list
    );

    if no_file {
        info!("Empty log-file name specified, no log file would be generated.")
    }

    use rucimp::strum::IntoEnumIterator;

    let all_possible_in_maps: Vec<_> = rucimp::modes::chain::config::InMapConfig::iter()
        .map(|x| ruci::utils::get_debug_head(&x))
        .collect();

    debug!("possible in maps: {}", all_possible_in_maps.join(", "));

    let all_possible_out_maps: Vec<_> = rucimp::modes::chain::config::OutMapConfig::iter()
        .map(|x| ruci::utils::get_debug_head(&x))
        .collect();

    debug!("possible out maps: {}", all_possible_out_maps.join(", "));

    guard
}

/// blocking
async fn start_engine(
    args: Args,
    file_name: String,
    #[cfg(feature = "api_server")] opts: Option<(
        api::server::Server,
        tokio::sync::mpsc::Receiver<()>,
        std::sync::Arc<ruci::net::GlobalTrafficRecorder>,
    )>,
) -> anyhow::Result<()> {
    match args.mode {
        Mode::C => {
            mode::chain::run(
                file_name,
                args,
                #[cfg(feature = "api_server")]
                opts,
            )
            .await?;
        }
    }
    Ok(())
}
