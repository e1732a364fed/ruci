/*!
 * module net defines some important parts for proxy.
 *
 * important parts: [`CID`], [`Network`], [`Addr`], [`ConnTrait`], [`Conn`], [`Stream`], [`GlobalTrafficRecorder`],
 *  and a cp mod for copying data between [`Conn`]


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
use smallvec::smallvec;
use smallvec::SmallVec;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
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

pub fn new_ordered_cid(last_id: &AtomicU32) -> u32 {
    last_id.fetch_add(1, Ordering::Relaxed) + 1
}

/// stream id ('c' for conn as convention)
///
/// default is CID::unit(0) which means no connection yet
///
/// CID is massively used in ruci. //首项为根id, 末项为末端stream的id
///
/// # Example
///
/// ```
/// use ruci::net::CID;
///
/// let cid = CID {
///       id_list: smallvec::smallvec![1, 2, 3],
///  };
///  println!("{}", cid)
/// ```
///
#[derive(Default, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CID {
    pub id_list: SmallVec<[u32; 2]>,
}

impl Display for CID {
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
                let last = self.id_list.len() - 1;
                for (i, u) in self.id_list.iter().enumerate() {
                    write!(f, "{}", u)?;
                    if i != last {
                        write!(f, "-")?;
                    }
                }
                Ok(())
            }
        }
    }
}

impl std::str::FromStr for CID {
    fn from_str(s: &str) -> anyhow::Result<Self> {
        let u = s.parse::<u32>();
        if let Ok(u) = u {
            return Ok(CID {
                id_list: smallvec![u],
            });
        }
        let v: Vec<_> = s.split('-').collect();

        let mut cid = CID::default();
        for s in v {
            let u = s.parse::<u32>();
            match u {
                Ok(u) => cid.push_num(u),
                Err(e) => return Err(anyhow!("CID can't parse from {}, {e}", s)),
            }
        }
        Ok(cid)
    }

    type Err = anyhow::Error;
}

impl CID {
    pub fn new(n: u32) -> Self {
        CID {
            id_list: smallvec![n],
        }
    }

    pub fn new_random() -> Self {
        Self::new(new_rand_cid())
    }

    /// new with ordered id
    pub fn new_ordered(last_id: &std::sync::atomic::AtomicU32) -> Self {
        Self::new(new_ordered_cid(last_id))
    }

    pub fn new_by_ogtr(ogtr: Option<Arc<GlobalTrafficRecorder>>) -> Self {
        match ogtr {
            Some(gtr) => Self::new_ordered(&gtr.last_connection_id),
            None => Self::new_random(),
        }
    }

    pub fn is_zero(&self) -> bool {
        match self.id_list.len() {
            0 => true,
            1 => self.id_list[0] == 0,
            _ => false,
        }
    }

    /// push will add a new number to self.
    ///
    /// if ogtr is given, it will add new id by the GlobalTrafficRecorder
    ///
    /// if None is give, it will generate a random id number
    ///
    /// # Example
    ///
    /// ```
    /// use ruci::net::CID;
    /// use ruci::net::GlobalTrafficRecorder;
    /// use std::sync::Arc;
    /// let ogtr = Some(Arc::new(GlobalTrafficRecorder::default()));
    ///
    /// let mut x = CID::new_by_ogtr(ogtr.clone());
    /// assert!(x == CID::new(1));
    /// x.push(ogtr.clone());
    /// assert!(x.id_list[0] == 1);
    ///
    /// x.push(ogtr);
    /// assert!(
    ///      x.id_list[0] == 1 || x.id_list[1] == 2
    /// );
    ///
    /// ```
    pub fn push(&mut self, ogtr: Option<Arc<GlobalTrafficRecorder>>) {
        let new_id_num = match ogtr.as_ref() {
            Some(gtr) => new_ordered_cid(&gtr.last_connection_id),
            None => new_rand_cid(),
        };

        self.push_num(new_id_num)
    }

    /// push_num add a new number to self.
    pub fn push_num(&mut self, new_id_num: u32) {
        self.id_list.push(new_id_num)
    }

    /// won't change self
    ///
    /// return the new CID
    pub fn clone_push(&self, ogtr: Option<Arc<GlobalTrafficRecorder>>) -> Self {
        let mut cid = self.clone();
        cid.push(ogtr);
        cid
    }

    /// won't change self
    ///
    /// return the popped value
    ///
    pub fn clone_pop(&self) -> Self {
        let mut cid = self.clone();
        cid.pop()
    }

    /// change self, collapse to Unit if the chain length=1 after pop
    ///
    /// return the popped value
    pub fn pop(&mut self) -> Self {
        let last = self.id_list.pop();
        match last {
            Some(last) => {
                match self.id_list.len() {
                    0 => {
                        *self = Self::new(0);
                    }
                    1 => {
                        let l = self.id_list.pop().expect("ok");
                        *self = Self::new(l)
                    }
                    _ => {}
                }

                Self::new(last)
            }
            None => Self::new(0),
        }
    }
}

pub type StreamGenerator = tokio::sync::mpsc::Receiver<MapResult>;

/// default is None
#[derive(Default)]
pub enum Stream {
    ///  raw ip / tcp / unix domain socket 等 目标 Addr 唯一的 情况
    Conn(Conn),

    //如果 从 raw ip 解析出了 ip 目标, 那么该ip流就是 AddrConn
    /// udp 的情况
    AddrConn(AddrConn),

    /// 比如： tcp listener.
    ///
    /// Receiver 中的元素为 MapResult, 是为了
    ///
    /// 方便传递其它信息, 如 RLAddr 由 MapResult.d 标识, 见
    ///
    /// [`crate::map::network::accept`]
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

    /// try shutdown the underlying stream. If there's no
    /// stream, no behavior.
    pub async fn try_shutdown(&mut self) -> Result<()> {
        match self {
            Stream::Conn(ref mut t) => t.shutdown().await?,
            Stream::AddrConn(ref mut c) => c.w.shutdown().await?,
            Stream::Generator(ref mut rx) => rx.close(),
            Stream::None => {}
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
/// 注意, 考虑在两个累加结果的Conn之间拷贝, 若用 [`mod@crate::net::cp`] 拷贝并给出 [`GlobalTrafficRecorder`],
/// 则它统计出的流量为 未经加密的原始流量, 实际流量一般会比原始流量大。要想用
/// [`mod@crate::net::cp`] 统计真实流量, 只能有一种情况, 那就是 tcp到tcp的直接拷贝,
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

/// AsyncConn 将 可异步读写的功能抽象出来。
///
/// [`TcpStream`] 也实现了 AsyncConn
///
pub trait AsyncConn: AsyncRead + AsyncWrite + Unpin + Send + Sync {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Sync> AsyncConn for T {}

/// the Trait is massively used in ruci
pub trait NamedConn: AsyncConn + Name {}
impl<T: AsyncConn + Name> NamedConn for T {}

impl crate::Name for TcpStream {
    fn name(&self) -> &str {
        "tcp_stream"
    }
}

#[cfg(unix)]
impl crate::Name for UnixStream {
    fn name(&self) -> &str {
        "unix_stream"
    }
}

/// an important type in ruci
pub type Conn = Box<dyn NamedConn>;
