/*!
Defines a struct that impl SuitConfigHolder+ MappersVec

通过套装, 我们得以将一串固定套路的代理传播链的配置扁平化

 */
pub mod config;

/// uses self-defined relay procedure
pub mod engine;

#[cfg(test)]
mod test;

use std::sync::Arc;

use async_trait::async_trait;
use ruci::map::tls;
use tokio::io;
use tokio::net::TcpListener;

use ruci::map::*;
use ruci::net::{self, Addr};

/// SuitConfigHolder : 一套完整的代理配置, 如从tcp到tls一直到socks5
///
/// 它定义了一个 rucimp::suit::config::LDConfig 持有者的应有的行为
///
/// 实现 Send 和 Sync 以在多线程环境中使用
pub trait SuitConfigHolder: Send + Sync {
    fn set_behavior(&mut self, b: ProxyBehavior);
    fn get_behavior(&self) -> ProxyBehavior;
    ///addr_str 是包含port的,用于 拨号, 但不包含network
    fn addr_str(&self) -> &str;
    fn addr(&self) -> Option<Addr>;

    ///config 所定义的所有的层的名称之和
    fn whole_name(&self) -> &str;

    ///代理层名
    fn protocol(&self) -> &str;
    fn get_config(&self) -> Option<&config::LDConfig>;
    fn set_config(&mut self, c: config::LDConfig) -> io::Result<()>;

    fn network(&self) -> &str {
        self.get_config()
            .map_or("tcp", |c| c.network.as_deref().unwrap_or("tcp"))
    }

    fn has_tls(&self) -> bool {
        self.get_config().map_or(false, |c| c.tls.unwrap_or(false))
    }
}

/// 一种 Mapper 的容器
pub trait MappersVec {
    fn get_mappers_vec(&self) -> Vec<Arc<MapperBox>>;

    fn push_mapper(&mut self, mapper: Arc<MapperBox>);
}

#[async_trait]
pub trait Suit: SuitConfigHolder + MappersVec {
    /// stop 停止监听, 同时移除一切因用户登录而生成的动态数据, 恢复到运行前的状态
    fn stop(&self) {}

    fn generate_upper_mappers(&mut self);
}

#[derive(Default, Debug)]
pub struct SuitStruct {
    pub addr_str: String,

    pub whole_name: String,

    pub config: config::LDConfig,

    pub in_mappers: Vec<Arc<MapperBox>>,
    pub out_mappers: Vec<Arc<MapperBox>>,

    addr: Option<Addr>,
    protocol_str: String,
    behavior: ProxyBehavior,
}

pub fn direct_suit() -> SuitStruct {
    let mut c = config::LDConfig::default();
    c.protocol.replace_range(.., "direct");
    SuitStruct::from(c)
}

impl SuitStruct {
    pub fn from(c: config::LDConfig) -> Self {
        let mut s = SuitStruct::default();
        match s.set_config(c) {
            Ok(_) => s,
            Err(e) => {
                panic!("config error, {}", e)
            }
        }
    }
}

impl SuitConfigHolder for SuitStruct {
    fn set_behavior(&mut self, b: ProxyBehavior) {
        self.behavior = b;
    }
    fn get_behavior(&self) -> ProxyBehavior {
        self.behavior
    }

    fn addr_str(&self) -> &str {
        self.addr_str.as_str()
    }
    fn addr(&self) -> Option<Addr> {
        self.addr.clone()
    }
    fn get_config(&self) -> Option<&config::LDConfig> {
        Some(&self.config)
    }
    fn set_config(&mut self, c: config::LDConfig) -> io::Result<()> {
        let ad = Addr::from_strs(
            c.network.as_deref().unwrap_or("tcp"),
            c.host.as_deref().unwrap_or_default(),
            c.ip.as_deref().unwrap_or_default(),
            c.port.unwrap_or(0),
        )
        .ok();

        if let Some(a) = ad.as_ref() {
            self.addr_str = a.get_addr_str();
            self.addr = ad;
        }
        //ad 为None可能是因为 配置文件本来就没写地址(direct的情况)

        self.protocol_str = c.protocol.clone();

        self.config = c;
        Ok(())
    }
    fn whole_name(&self) -> &str {
        &self.whole_name
    }

    fn protocol(&self) -> &str {
        &self.protocol_str
    }
}

impl MappersVec for SuitStruct {
    fn get_mappers_vec(&self) -> Vec<Arc<MapperBox>> {
        self.in_mappers.clone()
    }

    /// 添加 mapper 到尾部. 会自动推导出 whole_name.
    ///
    /// 可多次调用, 每次都会更新 whole_name
    fn push_mapper(&mut self, mapper: Arc<MapperBox>) {
        self.in_mappers.push(mapper);
        self.whole_name = self
            .in_mappers
            .iter()
            .map(|a| a.name())
            .collect::<Vec<_>>()
            .join("+");
    }
}

#[async_trait]
impl Suit for SuitStruct {
    fn generate_upper_mappers(&mut self) {
        let c = self.get_config().expect("has valid config").clone();

        match self.get_behavior() {
            ProxyBehavior::ENCODE => {
                if self.protocol_str != "direct" && !self.addr_str.is_empty() {
                    let mut a = network::Dialer::default();
                    a.dial_addr = net::Addr::from_network_addr_str(self.addr_str())
                        .expect("self addr str ok");
                    self.push_mapper(Arc::new(Box::new(a)));
                }
                if self.has_tls() {
                    let a = tls::client::Client::new(tls::client::ClientOptions {
                        domain: c.host.unwrap_or_default(),
                        is_insecure: c.insecure.unwrap_or_default(),
                        alpn: c.alpn,
                    });
                    self.push_mapper(Arc::new(Box::new(a)));
                }
            }
            ProxyBehavior::DECODE => {
                if self.has_tls() {
                    let so = tls::server::ServerOptions {
                        addr: "todo!()".to_string(),
                        cert: c.cert.expect("need cert file  in config").into(),
                        key: c.key.expect("need key file in config").into(),
                        alpn: c.alpn,
                    };
                    let sa = tls::server::Server::new(so);
                    self.in_mappers.push(Arc::new(Box::new(sa)));
                }
            }
            ProxyBehavior::UNSPECIFIED => {}
        }
    }
}
