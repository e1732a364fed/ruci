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

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const COMMON_DIRS: [&str; 5] = [
    "./",
    "ruci_config/",
    "resource/",
    "../resource/",
    "../../resource/",
];
