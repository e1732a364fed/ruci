use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerConfig {
    pub key_path: String,
    pub cert_path: String,
    pub listen_addr: String,
    pub alpn: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientConfig {
    pub server_addr: String,
    pub server_name: String,

    pub cert_path: Option<String>,
    pub alpn: Option<Vec<String>>,
    pub is_insecure: Option<bool>,
}
