/*!
Defines some proxying modes. Each mode defines a certain configuration format and an engine that runs it.
 */

use std::{str::FromStr, sync::Arc};

use data_source::DataSource;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tokio::sync::mpsc;
use tracing::{info, Level};

use crate::DEFAULT_LUA_CONFIG_FILE_NAME;

pub mod chain;

#[derive(Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Mode {
    /// Chain mode, which uses lua/json file
    #[default]
    Chain,
}

#[derive(Debug, Clone, Copy)]
pub struct LevelWrapper(pub tracing::Level);

impl Serialize for LevelWrapper {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = match self.0 {
            Level::ERROR => "ERROR",
            Level::WARN => "WARN",
            Level::INFO => "INFO",
            Level::DEBUG => "DEBUG",
            Level::TRACE => "TRACE",
        };
        serializer.serialize_str(s)
    }
}

impl<'de> Deserialize<'de> for LevelWrapper {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let level = match s.as_str() {
            "ERROR" => Level::ERROR,
            "WARN" => Level::WARN,
            // ... 其他变体
            _ => return Err(serde::de::Error::custom("Invalid level")),
        };
        Ok(LevelWrapper(level))
    }
}

#[derive(Debug)]
pub struct ParseLevelError {
    input: String,
}

impl std::fmt::Display for ParseLevelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Invalid log level: '{}'. Valid options are: ERROR, WARN, INFO, DEBUG, TRACE",
            self.input
        )
    }
}

impl std::error::Error for ParseLevelError {}

// 实现 FromStr
impl FromStr for LevelWrapper {
    type Err = ParseLevelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ERROR" => Ok(LevelWrapper(Level::ERROR)),
            "WARN" => Ok(LevelWrapper(Level::WARN)),
            "INFO" => Ok(LevelWrapper(Level::INFO)),
            "DEBUG" => Ok(LevelWrapper(Level::DEBUG)),
            "TRACE" => Ok(LevelWrapper(Level::TRACE)),
            _ => Err(ParseLevelError {
                input: s.to_string(),
            }),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct CoreArgs {
    /// choose the rucimp core mode
    pub mode: Mode,

    pub config_file_name: Option<String>,

    /// Basic config file. Can be of lua or json format.
    pub config_file_content: String,

    #[serde(skip)]
    pub data_source: Option<Arc<DataSource>>,

    #[serde(default)]
    pub in_memory: bool,

    pub log_level: Option<LevelWrapper>,

    /// Specify the log file prefix name.
    ///
    /// if empty string is given, no log file will be generated;
    ///
    /// if the flag is not given, log file will be generated with default name
    pub log_file: Option<String>,

    /// Specify the directory where log files would be in
    ///
    /// if empty string is given, log file will be generated in default folder
    ///
    /// if the flag is not given, log file will be generated in default folder
    pub log_dir: Option<String>,

    /// Use infinite dynamic chain that is written in the lua config file (the "Infinite"
    /// global variable must exist)
    #[cfg(any(feature = "lua", feature = "lua54"))]
    #[serde(default)]
    pub infinite: bool,

    /// Enable flux trace (might slow down performance)
    #[cfg(feature = "trace")]
    #[serde(default)]
    pub trace: bool,

    #[cfg(feature = "api_server")]
    #[serde(default)]
    pub api_server: bool,

    /// Default is "127.0.0.1:40681"
    #[cfg(feature = "api_server")]
    pub api_addr: Option<String>,
}

/// blocking until engine loop stopped
pub async fn run(
    args: CoreArgs,
    #[cfg(feature = "api_server")] api_server_opts: Option<&mut (
        crate::api::Server,
        tokio::sync::mpsc::Receiver<()>,
        Arc<ruci::net::GlobalTrafficRecorder>,
    )>,
) -> anyhow::Result<()> {
    let (mut e, r) = init_engine(
        args,
        #[cfg(feature = "api_server")]
        api_server_opts,
    )
    .await?;
    e.run_with_close_rx(r, true).await
}

/// returns the engine and the close_rx
pub async fn init_engine(
    args: CoreArgs,
    #[cfg(feature = "api_server")] mut api_server_opts: Option<&mut (
        crate::api::Server,
        tokio::sync::mpsc::Receiver<()>,
        Arc<ruci::net::GlobalTrafficRecorder>,
    )>,
) -> anyhow::Result<(
    crate::modes::chain::engine::Engine,
    Option<mpsc::Receiver<()>>,
)> {
    match args.mode {
        Mode::Chain => {
            info!("starting rucimp chain engine...");

            let mut e = crate::modes::chain::engine::Engine::new();

            use anyhow::Context;

            if let Some(ds) = args.data_source {
                e.data_source = ds.clone();
            }

            let file_name = args
                .config_file_name
                .unwrap_or(DEFAULT_LUA_CONFIG_FILE_NAME.to_string());

            if file_name.ends_with(".lua") {
                #[cfg(any(feature = "lua", feature = "lua54"))]
                {
                    if args.infinite {
                        e.init_lua_infinite_dynamic(args.config_file_content)?;
                    } else {
                        e.init_lua(args.config_file_content)?;
                    }
                }
            } else if file_name.ends_with(".json") {
                let c: chain::config::StaticConfig =
                    crate::serde_json::from_str(&args.config_file_content)
                        .context("json to StaticConfig failed")?;
                e.init_static(c).context("init static failed")?;
            } else {
                anyhow::bail!("unsupported file extension: {}", file_name);
            }

            #[cfg(feature = "api_server")]
            {
                if let Some(api_server_opts) = api_server_opts.as_mut() {
                    crate::api::setup_api_server_with_chain_engine(
                        &mut e,
                        #[cfg(feature = "trace")]
                        args.trace,
                        &mut api_server_opts.0,
                        api_server_opts.2.clone(),
                    )
                    .await;

                    let r = {
                        let (_t, r) = mpsc::channel(1);
                        std::mem::replace(&mut api_server_opts.1, r)
                    };
                    return Ok((e, Some(r)));
                }
            }

            Ok((e, None))
        }
    }
}
