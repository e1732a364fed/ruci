/*!
api 用于 在代理运行时，提供获取有关运行的相关信息。
 */

#[cfg(feature = "api_client")]
pub mod client;
#[cfg(feature = "api_server")]
pub mod server;

use super::*;

pub const DEFAULT_API_ADDR: &str = "127.0.0.1:40681";
