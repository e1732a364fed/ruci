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

pub use rucimp;
use serde::{Deserialize, Serialize};

use std::{
    env::{self, set_var},
    sync::Arc,
};

use clap::{Parser, Subcommand, ValueEnum};
use parking_lot::RwLock;
use rucimp::{modes::CoreArgs, DEFAULT_LUA_CONFIG_FILE_NAME};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
#[cfg(feature = "api_server")]
use utoipa;

#[derive(
    Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum,
)]
pub enum Mode {
    /// Chain mode, which uses lua/json file
    #[default]
    C,
}
impl Mode {
    fn to_core_mode(&self) -> rucimp::modes::Mode {
        match self {
            Mode::C => rucimp::modes::Mode::Chain,
        }
    }
}

/// ruci command line parameters:
#[derive(Parser, Clone, Default, Debug, Serialize, Deserialize)]
#[command(author = "e")]
#[command(version, about, long_about = None)]
pub struct Args {
    /// choose the rucimp core mode
    #[serde(default)]
    #[arg(short, long, value_enum, default_value_t = Mode::C )]
    pub mode: Mode,

    /// Basic config file. Can be of lua or json format.
    ///
    /// If the given string is a url, then the app will try to download the file first.
    #[arg(short, long, value_name = "FILE", default_value = DEFAULT_LUA_CONFIG_FILE_NAME)]
    #[serde(default)]
    pub config: String,

    /// If this arg is given, and the "config" arg is a url, then the app will try to download
    /// the config file but will not store it in the file system.
    ///
    /// This will cause the app to download the config file every time it runs.
    #[serde(default)]
    #[arg(long)]
    pub in_memory: bool,

    #[arg(short, long)]
    pub log_level: Option<rucimp::modes::LevelWrapper>,

    /// Specify the log file prefix name.
    ///
    /// if empty string is given, no log file will be generated;
    ///
    /// if the flag is not given, log file will be generated with default name
    #[arg(long)]
    pub log_file: Option<String>,

    /// Specify the directory where log files would be in
    ///
    /// if empty string is given, log file will be generated in default folder
    ///
    /// if the flag is not given, log file will be generated in default folder
    #[arg(long)]
    pub log_dir: Option<String>,

    /// Use infinite dynamic chain that is written in the lua config file (the "Infinite"
    /// global variable must exist)
    #[cfg(any(feature = "lua", feature = "lua54"))]
    #[serde(default)]
    #[arg(long)]
    pub infinite: bool,

    /// Enable flux trace (might slow down performance)
    #[cfg(feature = "trace")]
    #[serde(default)]
    #[arg(long)]
    pub trace: bool,

    #[cfg(feature = "api_server")]
    #[serde(default)]
    #[arg(short, long, default_value_t = false)]
    pub api_server: bool,

    /// Default is "127.0.0.1:40681"
    #[cfg(feature = "api_server")]
    #[arg(long)]
    pub api_addr: Option<String>,

    #[command(subcommand)]
    pub sub_cmds: Option<SubCommands>,
}

impl Args {
    fn to_core_args(&self) -> rucimp::modes::CoreArgs {
        CoreArgs {
            mode: self.mode.to_core_mode(),
            config_file_name: Some(self.config.clone()),
            config_file_content: "".to_string(),
            data_source: None,
            in_memory: self.in_memory,
            log_level: self.log_level,
            log_file: self.log_file.clone(),
            log_dir: self.log_dir.clone(),
            #[cfg(any(feature = "lua", feature = "lua54"))]
            infinite: self.infinite,
            #[cfg(feature = "trace")]
            trace: self.trace,
            #[cfg(feature = "api_server")]
            api_server: self.api_server,
            #[cfg(feature = "api_server")]
            api_addr: self.api_addr.clone(),
        }
    }
}

#[derive(Subcommand, Clone, Debug, Serialize, Deserialize)]
pub enum SubCommands {
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
}

/// blocking
pub async fn run_main() -> anyhow::Result<()> {
    let args = Args::parse();
    run_main_with_args(args).await
}
/// blocking

pub async fn run_main_with_json_args(json: &str) -> anyhow::Result<()> {
    let args = rucimp::serde_json::from_str(&json)?;
    run_main_with_args(args).await
}

#[test]
fn parse_json() -> anyhow::Result<()> {
    let args: Args = rucimp::serde_json::from_str(
        "{ \"api_server\": true, \"mode\": \"C\" , \"config\": \"local.lua\" }",
    )?;
    println!("{args:?}");
    Ok(())
}

#[derive(Debug, Default)]
pub enum State {
    #[default]
    NotRun,
    RunResult(anyhow::Result<()>),
    Running,
    RunningEngine,
    Stopped(anyhow::Result<()>),
}

use std::sync::OnceLock;
use tokio::runtime::Runtime;
static GLOBAL_RUNTIME: OnceLock<Runtime> = OnceLock::new();
pub static CORE_STATE: OnceLock<std::sync::Arc<parking_lot::Mutex<State>>> = OnceLock::new();

//注意，安卓运行 log 时若 没有 取消 日志文件输出，则会panic

/// non-blocking, using a new multithread tokio runtime.
#[no_mangle]
pub unsafe extern "C" fn c_run_main_with_json_args(
    json_content: *const std::ffi::c_char,
) -> *mut std::ffi::c_char {
    let json_content = std::ffi::CStr::from_ptr(json_content);
    let json_content_as_str = match json_content.to_str() {
        Ok(s) => s,
        Err(e) => {
            return std::ffi::CString::new(format!("to_str err: {}", e))
                .unwrap()
                .into_raw();
        }
    };

    let dr = &mut rucimp::serde_json::Deserializer::from_str(json_content_as_str);

    let args: Args = match serde_path_to_error::deserialize(dr) {
        Ok(args) => args,
        Err(e) => {
            return std::ffi::CString::new(format!("serde err: {e:#?}"))
                .unwrap()
                .into_raw();
        }
    };
    let rt = GLOBAL_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
    });

    CORE_STATE.get_or_init(|| Arc::new(parking_lot::Mutex::new(State::default())));

    let f = async {
        let r = run_main_with_args(args).await;

        std::mem::replace(&mut *CORE_STATE.get().unwrap().lock(), State::RunResult(r))
    };
    let _ = rt.spawn(f);

    let r = "ok".to_string();

    std::ffi::CString::new(r).unwrap().into_raw()
}

#[cfg(target_os = "android")]
#[allow(non_snake_case)]
pub mod android {
    use jni;

    use self::jni::objects::{JClass, JString};
    use self::jni::sys::jstring;
    use self::jni::JNIEnv;
    use super::*;

    /// real function
    #[no_mangle]
    pub unsafe extern "C" fn Java_com_example_myapplication_MainActivity_run(
        mut env: JNIEnv,
        _: JClass,
        java_pattern: JString,
    ) -> jstring {
        let r = c_run_main_with_json_args(
            env.get_string(&java_pattern)
                .expect("invalid pattern string")
                .as_ptr(),
        );

        let r_ptr = std::ffi::CString::from_raw(r);

        let output = env
            .new_string(r_ptr.to_str().unwrap())
            .expect("Couldn't create java string!");

        output.into_raw()
    }

    /// same function in another android kotlin class
    ///
    /// ```plaintext
    /// package com.ruci.android
    /// class Class1    {
    ///     init {
    ///         System.loadLibrary("ruci_cmd")
    ///     }
    ///     external fun run(input: String): String
    /// }
    /// ```
    #[no_mangle]
    pub unsafe extern "C" fn Java_com_ruci_android_Class1_run(
        env: JNIEnv,
        jc: JClass,
        java_pattern: JString,
    ) -> jstring {
        Java_com_example_myapplication_MainActivity_run(env, jc, java_pattern)
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_ruci_android_Class1_result(
        env: JNIEnv,
        _: JClass,
    ) -> jstring {
        let r = CORE_STATE.get().unwrap().lock();

        let x = std::ffi::CString::new(format!("{:?}", &*r)).unwrap();

        let output = env
            .new_string(x.to_str().unwrap())
            .expect("Couldn't create java string!");

        output.into_raw()
    }

    /// export an example function for testing
    #[no_mangle]
    pub unsafe extern "C" fn Java_com_example_myapplication_MainActivity_greeting(
        mut env: JNIEnv,
        _: JClass,
        java_pattern: JString,
    ) -> jstring {
        fn rust_greeting(to: *const std::ffi::c_char) -> *mut std::ffi::c_char {
            let c_str = unsafe { std::ffi::CStr::from_ptr(to) };
            let recipient = match c_str.to_str() {
                Err(_) => "there",
                Ok(string) => string,
            };

            std::ffi::CString::new("Hello ".to_owned() + recipient)
                .unwrap()
                .into_raw()
        }

        let world = rust_greeting(
            env.get_string(&java_pattern)
                .expect("invalid pattern string")
                .as_ptr(),
        );
        // Retake pointer so that we can use it below and allow memory to be freed when it goes out of scope.
        let world_ptr = std::ffi::CString::from_raw(world);
        let output = env
            .new_string(world_ptr.to_str().unwrap())
            .expect("Couldn't create java string!");

        output.into_raw()
    }
}

/// blocking
pub async fn run_main_with_args(args: Args) -> anyhow::Result<()> {
    let _g = log_setup(args.clone());

    match args.sub_cmds {
        None => {
            #[cfg(feature = "api_server")]
            {
                let mut api_server_opts: Option<(
                    rucimp::api::Server,
                    tokio::sync::mpsc::Receiver<()>,
                    std::sync::Arc<ruci::net::GlobalTrafficRecorder>,
                )> = None;

                let mut engine_started = false;
                let mut api_server_started = false;

                let epots = std::sync::Arc::new(Mutex::new(None));

                // 创建API扩展映射
                let api_extensions =
                    std::sync::Arc::new(RwLock::new(std::collections::HashMap::new()));

                // 注册Command API
                #[cfg(feature = "utils")]
                {
                    if let Err(e) = utils::register_command_apis(&mut api_extensions.clone()) {
                        warn!("Failed to register command APIs: {}", e);
                    }
                }

                if args.api_server {
                    let mut opts = rucimp::api::Server::new(
                        args.api_addr.clone(),
                        epots.clone(),
                        Some(api_extensions),
                        #[cfg(feature = "utils")]
                        Some(<utils::ApiDoc as utoipa::OpenApi>::openapi()),
                        #[cfg(not(feature = "utils"))]
                        None,
                    )
                    .await;
                    api_server_started = true;

                    if args.config == DEFAULT_LUA_CONFIG_FILE_NAME {
                        api_server_opts = Some(opts);
                    } else {
                        start_engine(args.clone(), Some(&mut opts)).await?;
                        engine_started = true;
                    }
                }
                if !engine_started {
                    if api_server_started && args.config == DEFAULT_LUA_CONFIG_FILE_NAME {
                        // 如果api server 给出，且 配置 文件为默认值，则不直接运行 engine, 而是只启动 api_server, 并在
                        // api_server 中进行监听，等待 启动engine 的 api 被调用

                        if let Some(opts) = api_server_opts {
                            let _ = epots.lock().await.insert(opts);

                            info!("api server started, running api...");

                            let _ = std::mem::replace(
                                &mut *CORE_STATE.get().unwrap().lock(),
                                State::Running,
                            );

                            rucimp::utils::wait_close_sig().await?
                        }
                    } else {
                        start_engine(args.clone(), None).await?;
                    }
                }
            }
            #[cfg(not(feature = "api_server"))]
            start_engine(args.clone()).await?;
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
                let r = utils::deal_cmds(command).await;
                if let Err(r) = r {
                    tracing::warn!("{:#}", r)
                }
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
        l.0.as_str()
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

    #[cfg(debug_assertions)]
    let console_layer = fmt::layer()
        .with_line_number(true)
        .with_writer(std::io::stderr);

    #[cfg(not(debug_assertions))]
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
        std::env::var(RL).unwrap_or_else(|_| String::new())
    );

    #[allow(unused_mut)]
    let mut features_list: Vec<&str> = vec![
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
pub async fn start_engine(
    args: Args,
    #[cfg(feature = "api_server")] api_server_opts: Option<&mut (
        rucimp::api::Server,
        tokio::sync::mpsc::Receiver<()>,
        std::sync::Arc<ruci::net::GlobalTrafficRecorder>,
    )>,
) -> anyhow::Result<()> {
    match args.mode {
        Mode::C => {
            let (fc, ds) =
                mode::chain::get_config_file(&mut args.config.clone(), args.in_memory).await?;

            let mut args = args.to_core_args();
            args.config_file_content = fc;
            args.data_source = Some(std::sync::Arc::new(ds));

            let _ = std::mem::replace(&mut *CORE_STATE.get().unwrap().lock(), State::RunningEngine);

            let r = rucimp::modes::run(
                args,
                #[cfg(feature = "api_server")]
                api_server_opts,
            )
            .await;

            match &r {
                Ok(_) => {
                    let _ = std::mem::replace(
                        &mut *CORE_STATE.get().unwrap().lock(),
                        State::Stopped(Ok(())),
                    );
                }
                Err(e) => {
                    let _ = std::mem::replace(
                        &mut *CORE_STATE.get().unwrap().lock(),
                        State::Stopped(Err(anyhow::anyhow!("{e:?}"))),
                    );
                }
            };

            r?
        }
    }

    Ok(())
}
