/*!
Defines common parts for various quic implementations.
 */

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerConfig {
    pub key: String,
    pub cert: String,
    pub listen_addr: String,
    pub alpn: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientConfig {
    pub server_addr: String,
    pub server_name: String,

    pub cert: Option<String>,
    pub alpn: Option<Vec<String>>,
    pub insecure: Option<bool>,
}
