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

#[cfg(feature = "api_server")]
pub mod api;

pub use base64;
pub use serde_json;
pub use strum;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Common directories to search for configuration files
pub const COMMON_DIRS: [&str; 12] = [
    "./",
    "ruci_config/",
    "resource/",
    "dev_res/",
    "../dev_res/",
    "../../dev_res/",
    "../resource/",
    "../resource/lua_examples/local",
    "../resource/lua_examples/remote",
    "../../resource/",
    "../../resource/lua_examples/local",
    "../../resource/lua_examples/remote",
];

/// Default name for Lua configuration files
pub const DEFAULT_LUA_CONFIG_FILE_NAME: &str = "local.lua";

pub const DEFAULT_API_ADDR: &str = "127.0.0.1:40681";
