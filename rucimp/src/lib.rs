/*!

 定义了 一些配置格式, 以及以这些配置格式运行代理的方法

*/
pub mod chain;
pub mod map;
pub mod suit;
pub mod user;

pub mod example_common;

#[cfg(feature = "route")]
pub mod route;

pub const VERSION: &str = "0.0.1";

pub const COMMON_DIRS: [&'static str; 4] = ["./", "ruci_config/", "resource/", "../resource/"];
