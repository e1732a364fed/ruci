/*!

定义了 一些 【模式】, 一些 配置格式, 以及以这些配置格式运行相应 【模式】的代理的方法

以及一些对 ruci 的扩展

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
