/*!
Defines some `mode`s and some extension maps and related facilities for ruci.

*/
pub mod map;
pub mod modes;
pub mod net;
pub mod user;

pub mod utils;

#[cfg(feature = "route")]
pub mod route;

#[cfg(feature = "toml")]
pub use toml;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const COMMON_DIRS: [&str; 6] = [
    "./",
    "ruci_config/",
    "resource/",
    "dev_res/",
    "../resource/",
    "../../resource/",
];

pub const DEFAULT_LUA_CONFIG_FILE_NAME: &str = "local.lua";
