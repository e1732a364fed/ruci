/*!
 * module net defines some important parts for proxy.
 *
 * important parts: CID, Network, Addr, ConnTrait, Conn, Stream, GlobalTrafficRecorder,
 *  and a cp mod for copying data between Conn

 enums:
CID, Network ,Addr ,Stream

 structs:
 CIDChain, GlobalTrafficRecorder,

trait: ConnTrait

type: Conn

function: cp

*/
pub mod addr;
pub mod addr_conn;
pub mod helpers;
pub mod http;
pub mod listen;
pub mod udp;

pub mod cp;

pub use addr::*;
pub use cp::*;

#[cfg(feature = "tun")]
pub mod tun;

#[cfg(test)]
mod test;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use futures::pin_mut;
use futures::{io::Error, FutureExt};
use rand::Rng;
use serde::Deserialize;
use serde::Serialize;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::vec;
use std::{fmt::Debug, net::Ipv4Addr};
use std::{
    fmt::{Display, Formatter},
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::Arc,
};
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::net::UdpSocket;
#[cfg(unix)]
use tokio::net::UnixStream;
use tracing::debug;

// #[derive(Default)]
// pub enum GenerateCIDBehavior {
//     #[default]
//     Random,
//     Ordered,
// }

pub fn new_rand_cid() -> u32 {
    const ID_RANGE_START: u32 = 100_000;

    rand::thread_rng().gen_range(ID_RANGE_START..=ID_RANGE_START * 10 - 1)
}

pub fn new_ordered_cid(lastid: &AtomicU32) -> u32 {
    lastid.fetch_add(1, Ordering::Relaxed) + 1
}

/// # Example
///
/// ```
/// use ruci::net::CIDChain;
///
/// let cc = CIDChain {
///       id_list: vec![1, 2, 3],
///  };
///  println!("{}", cc)
/// ```
///
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CIDChain {
    pub id_list: Vec<u32>, //首项为根id, 末项为末端stream的id
}
impl CIDChain {
    pub fn str(&self) -> String {
        match (self.id_list).len() {
            0 => String::from("_"),
            1 => self
                .id_list
                .first()
                .expect("get first element of cid")
                .to_string(),
            _ => {
                let v: Vec<_> = self.id_list.iter().map(|id| id.to_string()).collect();
                v.join("-")
            }
        }
    }
}

impl Display for CIDChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.id_list).len() {
            0 => {
                write!(f, "_")
            }
            1 => {
                write!(
                    f,
                    "{}",
                    self.id_list.first().expect("get first element of cid")
                )
            }
            _ => {
                let v: Vec<_> = self.id_list.iter().map(|id| id.to_string()).collect();
                let s = v.join("-");
                write!(f, "{}", s)
            }
        }
    }
}

/// stream id ('c' for conn as convention)
///
/// default is CID::Unit(0) which means no connection yet
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CID {
    Unit(u32),
    Chain(CIDChain),
}

impl Default for CID {
    fn default() -> Self {
        CID::Unit(0)
    }
}

impl Display for CID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CID::Unit(u) => write!(f, "{u}"),
            CID::Chain(c) => Display::fmt(c, f),
        }
    }
}

impl std::str::FromStr for CID {
    fn from_str(s: &str) -> anyhow::Result<Self> {
        let u = s.parse::<u32>();
        if let Ok(u) = u {
            return Ok(CID::Unit(u));
        }
        Err(anyhow!("cid can't parse from {}", s))
    }

    type Err = anyhow::Error;
}

impl CID {
    /// show the number(s) only
    pub fn str(&self) -> String {
        match self {
            CID::Unit(u) => u.to_string(),
            CID::Chain(c) => c.str(),
        }
    }
    pub fn new_random() -> CID {
        CID::Unit(new_rand_cid())
    }

    /// new with ordered id
    pub fn new_ordered(lastid: &std::sync::atomic::AtomicU32) -> CID {
        let li = new_ordered_cid(lastid);
        CID::Unit(li)
    }

    pub fn new_by_ogtr(oti: Option<Arc<GlobalTrafficRecorder>>) -> CID {
        match oti {
            Some(ti) => CID::new_ordered(&ti.last_connection_id),
            None => CID::new_random(),
        }
    }

    /// is Unit(0) or can collapse to it
    pub fn is_zero(&self) -> bool {
        match self {
            CID::Unit(u) => *u == 0,
            CID::Chain(chain) => match chain.id_list.len() {
                0 => true,
                1 => chain.id_list[0] == 0,
                _ => false,
            },
        }
    }

    /// change self
    ///
    /// # Example
    ///
    /// ```
    /// use ruci::net::CID;
    /// use ruci::net::GlobalTrafficRecorder;
    /// use std::sync::Arc;
    /// let oti = Some(Arc::new(GlobalTrafficRecorder::default()));
    ///
    /// let mut x = CID::new_by_ogtr(oti.clone());
    /// assert!(matches!(x, CID::Unit(1)));
    /// x.push(oti.clone());
    /// assert!(matches!(x.clone(), CID::Chain(chain) if chain.id_list[0] == 1));
    ///
    /// x.push(oti);
    /// assert!(
    ///     matches!(x.clone(), CID::Chain(chain) if chain.id_list[0] == 1 || chain.id_list[1] == 2)
    /// );
    ///
    /// ```
    pub fn push(&mut self, oti: Option<Arc<GlobalTrafficRecorder>>) {
        let newidnum = match oti.as_ref() {
            Some(ti) => new_ordered_cid(&ti.last_connection_id),
            None => new_rand_cid(),
        };

        match self {
            CID::Unit(u) => {
                let x = *u;
                if x == 0 {
                    *self = CID::Unit(newidnum);
                } else {
                    *self = CID::Chain(CIDChain {
                        id_list: vec![x, newidnum],
                    })
                }
            }
            CID::Chain(c) => c.id_list.push(newidnum),
        };
    }
    /// won't change self
    pub fn clone_push(&self, oti: Option<Arc<GlobalTrafficRecorder>>) -> Self {
        let mut cid = self.clone();
        cid.push(oti);
        cid
    }

    /// won't change self
    pub fn clone_pop(&self) -> CID {
        let mut cid = self.clone();
        cid.pop()
    }

    /// change self, collapse to Unit if the chain lenth=1 after pop
    pub fn pop(&mut self) -> CID {
        match self {
            CID::Unit(u) => {
                let u = *u;
                *self = CID::Unit(0);
                CID::Unit(u)
            }
            CID::Chain(chain) => {
                let last = chain.id_list.pop();
                match last {
                    Some(last) => {
                        match chain.id_list.len() {
                            0 => {
                                *self = CID::Unit(0);
                            }
                            1 => {
                                let l = chain.id_list.pop().expect("ok");
                                *self = CID::Unit(l)
                            }
                            _ => {}
                        }

                        CID::Unit(last)
                    }
                    None => CID::Unit(0),
                }
            }
        }
    }
}

pub type StreamGenerator = tokio::sync::mpsc::Receiver<MapResult>;

/// default is None
#[derive(Default)]
pub enum Stream {
    ///  rawip / tcp / unix domain socket 等 目标 Addr 唯一的 情况
    Conn(Conn),

    //如果 从 rawip 解析出了 ip 目标, 那么该ip流就是 AddrConn
    /// udp 的情况
    AddrConn(AddrConn),

    /// 比如： tcp listener. Receiver 中的元素为 MapResult, 是为了
    /// 方便传递其它信息, 如peer_addr 由 MapResult.a 标识
    Generator(StreamGenerator),

    #[default]
    None,
}

impl Debug for Stream {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self, f)
    }
}

impl Display for Stream {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

impl Stream {
    pub fn to_str(&self) -> &str {
        match &self {
            Stream::Conn(c) => c.name(),
            Stream::AddrConn(ac) => ac.name(),
            Stream::Generator(_) => "SomeStreamGenerator",
            Stream::None => "NoStream",
        }
    }
    pub fn c(c: Conn) -> Self {
        Stream::Conn(c)
    }
    pub fn u(u: AddrConn) -> Self {
        Stream::AddrConn(u)
    }
    pub fn g(g: StreamGenerator) -> Self {
        Stream::Generator(g)
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Stream::None)
    }
    pub fn is_some(&self) -> bool {
        !matches!(self, Stream::None)
    }
    pub fn is_generator(&self) -> bool {
        matches!(self, Stream::Generator(_))
    }
    pub fn is_none_or_generator(&self) -> bool {
        matches!(self, Stream::None) || matches!(self, Stream::Generator(_))
    }
    pub async fn try_shutdown(self) -> Result<()> {
        if let Stream::Conn(mut t) = self {
            t.shutdown().await?
        } else if let Stream::AddrConn(mut c) = self {
            c.w.shutdown().await?
        }
        Ok(())
    }

    pub fn try_unwrap_tcp(self) -> Result<Conn> {
        if let Stream::Conn(t) = self {
            return Ok(t);
        }
        Err(anyhow!("not tcp"))
    }

    pub fn try_unwrap_tcp_ref(&self) -> Result<&Conn> {
        if let Stream::Conn(t) = self {
            return Ok(t);
        }
        Err(anyhow!("not tcp"))
    }

    pub fn try_unwrap_udp(self) -> Result<AddrConn> {
        if let Stream::AddrConn(t) = self {
            return Ok(t);
        }
        Err(anyhow!("not udp"))
    }

    pub async fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Conn(conn) => conn.write(buf).await,
            Stream::AddrConn(ac) => {
                let x = ac.default_write_to.as_ref();
                match x {
                    Some(ta) => ac.w.write(buf, ta).await,
                    None => Err(std::io::Error::other(
                        "stream can't write udp directly without a target",
                    )),
                }
            }
            _ => Err(std::io::Error::other("stream is not tcp/udp, can't write")),
        }
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        match self {
            Stream::Conn(conn) => conn.write_all(buf).await,
            Stream::AddrConn(ac) => {
                let x = ac.default_write_to.as_ref();
                match x {
                    Some(ta) => ac.w.write(buf, ta).await.map(|_| ()),
                    None => Err(std::io::Error::other(
                        "stream can't write udp directly without a target",
                    )),
                }
            }
            _ => Err(std::io::Error::other("stream is not tcp/udp, can't write")),
        }
    }
}

use crate::map::MapResult;
use crate::Name;

use self::addr_conn::AddrConn;
use self::addr_conn::AsyncWriteAddrExt;

/// 用于全局状态监视和流量统计
///
/// ## About Real Data Traffic and Original Traffic
///
/// 注意, 考虑在两个累加结果的Conn之间拷贝, 若用 ruci::net::cp 拷贝并给出 GlobalTrafficRecorder,
/// 则它统计出的流量为 未经加密的原始流量, 实际流量一般会比原始流量大。要想用
/// ruci::net::cp 统计真实流量, 只能有一种情况, 那就是 tcp到tcp的直接拷贝,
/// 不使用累加器。
///
/// 一种统计正确流量的办法是, 将 Tcp连接包装一层专门记录流量的层, 见 counter 模块
///
#[derive(Debug, Default)]
pub struct GlobalTrafficRecorder {
    pub last_connection_id: AtomicU32,

    pub alive_connection_count: AtomicU32,

    /// total downloaded bytes since start
    pub db: AtomicU64,

    /// total uploaded bytes
    pub ub: AtomicU64,
}

/// ConnTrait 将 可异步读写的功能抽象出来。TcpStream 也实现了 ConnTrait
/// 这是一个很重要的 Trait
pub trait ConnTrait: AsyncRead + AsyncWrite + Unpin + Send + Sync + Name {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Sync + Name> ConnTrait for T {}

impl crate::Name for TcpStream {
    fn name(&self) -> &str {
        "tcpstream"
    }
}

#[cfg(unix)]
impl crate::Name for UnixStream {
    fn name(&self) -> &str {
        "unixstream"
    }
}

/// an important type in ruci
pub type Conn = Box<dyn ConnTrait>;
