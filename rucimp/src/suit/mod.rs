/*!
 * suit 模块定义了一个 实现 SuitConfigHolder, ruci::proxy::AddersVec  的结构
 *
 * 通过套装，我们得以将一串代理传播链扁平化
 *
 */
pub mod config;
pub mod engine;

/// mock of engine, but uses listen_ser2 -> listen_tcp2 -> handle_conn_clonable
///
/// `Arc<Suit>` to  `&'static dyn Suit`
pub mod engine2;

#[cfg(test)]
mod test;

use async_trait::async_trait;
use log::Level::Debug;
use log::{debug, info, log_enabled};
use ruci::map::tls;
use ruci::relay;
use std::sync::Arc;
use tokio::io;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use ruci::map::*;
use ruci::net::{self, Addr};

/// SuitConfigHolder ：一套完整的代理配置，如从tcp到tls一直到socks5
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
    /// stop 停止监听，同时移除一切因用户登录而生成的动态数据, 恢复到运行前的状态
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
    /// 可多次调用，每次都会更新 whole_name
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
                        cert: c.cert.unwrap().into(),
                        key: c.key.unwrap().into(),
                    };
                    let sa = tls::server::Server::new(so);
                    self.inmappers.push(Box::new(sa));
                }
            }
            ProxyBehavior::UNSPECIFIED => {}
        }
    }
}

/// 阻塞监听 ins。
///
/// 确保调用 listen_ser 前，ins 和 outc 的
/// generate_...adders 方法被调用过了
pub async fn listen_ser(
    ins: Arc<dyn Suit>,
    outc: Arc<dyn Suit>,
    oti: Option<Arc<net::TransmissionInfo>>,
    shutdown_rx: oneshot::Receiver<()>,
) -> io::Result<()> {
    let n = ins.network();
    match n {
        "tcp" => {
            if outc.network() != "tcp" {
                panic!(
                    "not implemented for dialing network other than tcp: {}",
                    outc.network()
                )
            }
            listen_tcp(ins, outc, oti, shutdown_rx).await
        }
        _ => Err(io::Error::other(format!(
            "such network not supported: {}",
            n
        ))),
    }
}

pub async fn listen_ser2(
    ins: &'static dyn Suit,
    outc: &'static dyn Suit,
    oti: Option<Arc<net::TransmissionInfo>>,
    shutdown_rx: oneshot::Receiver<()>,
) -> io::Result<()> {
    let n = ins.network();
    match n {
        "tcp" => {
            if outc.network() != "tcp" {
                panic!(
                    "not implemented for dialing network other than tcp: {}",
                    outc.network()
                )
            }
            listen_tcp2(ins, outc, oti, shutdown_rx).await
        }
        _ => Err(io::Error::other(format!(
            "such network not supported: {}",
            n
        ))),
    }
}

/// 阻塞监听 ins tcp。
async fn listen_tcp(
    ins: Arc<dyn Suit>,
    outc: Arc<dyn Suit>,
    oti: Option<Arc<net::TransmissionInfo>>,
    shutdown_rx: oneshot::Receiver<()>,
) -> io::Result<()> {
    let laddr = ins.addr_str().to_string();
    let wn = ins.whole_name().to_string();
    info!("start listen tcp {}, {}", laddr, wn);

    let listener = TcpListener::bind(laddr.clone()).await?;

    let clone_oti = move || oti.clone();
    let insc = move || ins.clone();
    let outcc = move || outc.clone();

    tokio::select! {
        r = async {
            loop {
                let r = listener.accept().await;
                if r.is_err(){

                    break;
                }
                let (tcpstream, raddr) = r.unwrap();

                let laddr = laddr.clone();
                let ti = clone_oti();
                let ins = insc();
                let outc = outcc();

                tokio::spawn(async move {
                    if log_enabled!(Debug) {
                        debug!("new tcp in, laddr:{}, raddr: {:?}", laddr, raddr);
                    }

                    let _ = relay::conn::handle_conn(
                        Box::new(tcpstream),
                        ins.whole_name(),
                        outc.whole_name(),
                        raddr.to_string(),
                        "tcp",
                        ins.get_mappers_vec().iter(),
                        outc.get_mappers_vec().iter(),
                        outc.addr(),
                        ti,
                    )
                    .await;
                });

            }

            Ok::<_, io::Error>(())
        } => {
            r

        }
        _ = shutdown_rx => {
            info!("terminating accept loop, {}",wn);
            Ok(())
        }
    }
}

pub struct FixedOutSelector<'a, T>
where
    T: Iterator<Item = &'a MapperBox> + Clone + Send,
{
    pub mappers: T,
    pub addr: Option<net::Addr>,
}

impl<'a, T> relay::conn::OutSelector<'a, T> for FixedOutSelector<'a, T>
where
    T: Iterator<Item = &'a MapperBox> + Clone + Send + Sync,
{
    fn select(&self, _params: Vec<Option<AnyData>>) -> (T, Option<net::Addr>) {
        (self.mappers.clone(), self.addr.clone())
    }
}

/// 阻塞监听 ins tcp。
async fn listen_tcp2(
    ins: &'static dyn Suit,
    outc: &'static dyn Suit,
    oti: Option<Arc<net::TransmissionInfo>>,
    shutdown_rx: oneshot::Receiver<()>,
) -> io::Result<()> {
    let laddr = ins.addr_str().to_string();
    let wn = ins.whole_name().to_string();
    info!("start listen tcp {}, {}", laddr, wn);

    let listener = TcpListener::bind(laddr.clone()).await?;

    let clone_oti = move || oti.clone();

    let selector = FixedOutSelector {
        mappers: outc.get_mappers_vec().iter(),
        addr: outc.addr(),
    };
    let f = Box::new(selector);
    let f = Box::leak(f);

    tokio::select! {
        r = async {
            loop {
                let r = listener.accept().await;
                if r.is_err(){

                    break;
                }
                let (tcpstream, raddr) = r.unwrap();

                let ti = clone_oti();
                if log_enabled!(Debug) {
                    debug!("new tcp in, laddr:{}, raddr: {:?}", laddr, raddr);
                }

                tokio::spawn( relay::conn::handle_conn_clonable(
                        Box::new(tcpstream),
                        ins.get_mappers_vec().iter(),
                        f,
                        ti,
                    )
                );
            }

            Ok::<_, io::Error>(())
        } => {
            r

        }
        _ = shutdown_rx => {
            info!("terminating accept loop, {} ",wn );
            Ok(())
        }
    }
}
