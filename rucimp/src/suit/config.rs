pub mod adapter;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Config {
    pub listen: Vec<LDConfig>,
    pub dial: Vec<LDConfig>,
}

impl Config {
    //panic if the toml_str is invalid
    pub fn from_toml(toml_str: &str) -> Self {
        toml::from_str(toml_str).unwrap()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserPass {
    pub user: String,
    pub pass: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum FallbackItem {
    Addr(String),
    Port(i64),
}

/// LDConfig 是 listen 和 dial 共用的 配置结构，配置是扁平化的
///
/// tls 的最低版本号配置填在这里：
///extra = { tls_minv = "1.2" }, 或 extra.tls_minv = "1.2"
///
///
#[allow(unused)]
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct LDConfig {
    pub tag: Option<String>,
    pub host: Option<String>,
    pub ip: Option<String>,
    pub network: Option<String>, //默认为tcp
    pub extra: Option<HashMap<String, toml::Value>>,
    pub port: Option<u16>, //若Network不为 unix , 则port项必填
    pub xver: Option<u8>,  //可选，只能为0/1/2. 若不为0, 则表示使用 pub PROXY protocol 协议头
    pub tls: Option<bool>,
    pub cert: Option<String>,   //tls server
    pub key: Option<String>,    //tls server
    pub insecure: Option<bool>, //tls 是否安全
    pub alpn: Option<Vec<String>>,
    pub protocol: String,             //代理层协议名
    pub uuid: Option<String>,         // protocol 的用户识别
    pub version: Option<u16>,         // protocol 的 version
    pub encrypt_algo: Option<String>, //protocol 内部的加密算法选择

    pub number_arg: Option<i64>, //for math adder

    /// listen part
    //noroute 意味着 传入的数据 不会被分流，一定会被转发到默认的 dial
    // 这一项是针对 分流功能的. 如果不设noroute, 则所有listen 得到的流量都会被 试图 进行分流
    pub noroute: Option<bool>,
    pub fallback: Option<FallbackItem>, //默认回落的地址，一般可为 ip:port,数字port or unix socket的文件名

    pub users: Option<Vec<UserPass>>,

    /// dial part
    pub send_through: Option<String>, //用于发送数据的 IP 地址, 可以是ip:port, 或者 tcp:ip:port\nudp:ip:port
}

#[cfg(test)]
mod test {
    use toml::Table;

    use crate::config::Config;

    #[test]
    fn test() {
        let value = "foo = 'bar'".parse::<Table>().unwrap();

        assert_eq!(value["foo"].as_str(), Some("bar"));

        let toml_str = r#"
        [[listen]]
        protocol = "socks5"
        host = "127.0.0.1"
        port = 1234
    
        [[dial]]
        protocol = "vmess"
        tag = "tag1"
        uuid = "11223344-5566-7788-9900-112233445566"
        host = "111.222.333.444"
        port = 5678
        extra = { vmess_security = "aes-128-gcm" }
        "#;

        let decoded: Config = toml::from_str(toml_str).unwrap();
        println!("{:#?}", decoded);
    }
}
