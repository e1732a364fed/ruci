/*!
 * suit 模块定义了一个 实现 SuitConfigHolder, ruci::proxy::AddersVec  的结构
 *
 * 通过套装, 我们得以将一串代理传播链扁平化
 *
 */
pub mod config;

/// uses self-defined relay procedure, which is similar to what's in verysimple project.
pub mod engine;

/// mock of engine, but relay procedure is listen_ser2 -> listen_tcp2
///   -> ruci::relay::handle_conn_clonable
///
/// `Arc<Suit>` to  `&'static dyn Suit`
pub mod engine2;

#[cfg(test)]
mod test;

use async_trait::async_trait;
use log::log_enabled;
use log::Level::Debug;
use ruci::map::tls;
use ruci::relay;
use tokio::io;
use tokio::net::TcpListener;

use ruci::map::*;
use ruci::net::{self, Addr};

/// SuitConfigHolder ：一套完整的代理配置, 如从tcp到tls一直到socks5
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
    fn get_mappers_vec(&self) -> &Vec<MapperBox>;

    fn push_mapper(&mut self, adder: MapperBox);
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

    pub inmappers: Vec<MapperBox>,
    pub outmappers: Vec<MapperBox>,

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
    fn get_mappers_vec(&self) -> &Vec<MapperBox> {
        &self.inmappers
    }

    /// 添加 mapper 到尾部。会自动推导出 whole_name.
    ///
    /// 可多次调用, 每次都会更新 whole_name
    fn push_mapper(&mut self, mapper: MapperBox) {
        self.inmappers.push(mapper);
        self.whole_name = self
            .inmappers
            .iter()
            .map(|a| a.name())
            .collect::<Vec<_>>()
            .join("+");
    }
}

#[async_trait]
impl Suit for SuitStruct {
    fn generate_upper_mappers(&mut self) {
        let c = self.get_config().unwrap().clone();

        match self.get_behavior() {
            ProxyBehavior::ENCODE => {
                if self.has_tls() {
                    let a = tls::client::Client::new(
                        c.host.unwrap_or_default().as_str(),
                        c.insecure.unwrap_or(false),
                    );
                    self.push_mapper(Box::new(a));
                }
            }
            ProxyBehavior::DECODE => {
                if self.has_tls() {
                    let so = tls::server::ServerOptions {
                        addr: "todo!()".to_string(),
                        cert: c.cert.expect("need cert file  in config").into(),
                        key: c.key.expect("need key file in config").into(),
                    };
                    let sa = tls::server::Server::new(so);
                    self.inmappers.push(Box::new(sa));
                }
            }
            ProxyBehavior::UNSPECIFIED => {}
        }
    }
}
