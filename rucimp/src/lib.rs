/*!
Defines proxy modes and extension maps for the ruci framework.

This crate provides concrete implementations of the abstractions defined in ruci.
*/

pub mod map;
pub mod modes;
pub mod net;
pub mod user;

pub mod utils;

pub mod route;

pub use serde_json;
pub use strum;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Common directories to search for configuration files
pub const COMMON_DIRS: [&str; 8] = [
    "./",
    "ruci_config/",
    "resource/",
    "dev_res/",
    "../dev_res/",
    "../../dev_res/",
    "../resource/",
    "../../resource/",
];

/// Default name for Lua configuration files
pub const DEFAULT_LUA_CONFIG_FILE_NAME: &str = "local.lua";
