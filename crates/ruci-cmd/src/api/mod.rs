#[cfg(feature = "api_client")]
pub mod client;
#[cfg(feature = "api_server")]
pub mod server;

use super::*;

pub const DEFAULT_API_ADDR: &'static str = "127.0.0.1:40681";
