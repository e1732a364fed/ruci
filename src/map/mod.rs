/*!
module map defines some important traits for proxy

几个关键部分: AnyData, MapParams, MapResult, Mapper, 和 acc 模块

ruci 包中实现 Mapper 的模块有: math, counter,stdio, network, socks5,http, socks5http, trojan,  tls

ruci 将任意代理行为分割成若干个不可再分的
流映射函数, function map(stream1, args...)-> (stream2, useful_data...)


流映射函数 的提供者 在本包中被命名为 "Mapper", 映射的行为叫 "maps"

在本包中， 有时使用 “加法” 来指代 映射。以“累加”来指代迭代映射。
即有时本包会以 "adder" 指代 Mapper

按顺序执行若干映射函数 的迭代行为 被ruci称为“累加”, 执行者被称为 “累加器”

之所以叫加法，是因为代理的映射只会增加信息（熵），不会减少信息

按代理的方向, 逻辑上分 Encode 和 Decode 两种, 以 maps 方法的 behavior 参数加以区分.


一个完整的代理链 是由 【生成 映射函数 的迭代器】生成的, 其在 acc 和 acc2 模块中有定义


*/

pub mod acc;

pub mod counter;
pub mod fileio;
pub mod http;
pub mod math;
pub mod network;
pub mod socks5;
pub mod socks5http;
pub mod stdio;
pub mod tls;
pub mod trojan;

#[cfg(test)]
mod test;

use crate::{
    net::{self, *},
    *,
};
use anyhow::anyhow;
use async_trait::async_trait;
use bytes::BytesMut;
use dyn_clone::DynClone;
use log::{info, log_enabled, warn};
use tokio::sync::oneshot;
use typed_builder::TypedBuilder;

use std::{
    fmt::Debug,
    sync::{
        atomic::{AtomicI64, AtomicU64},
        Arc,
    },
};

use self::addr_conn::AddrConn;

/// a data type represents what a Mapper might generate.
///
/// The data must be clonable
///
#[derive(Debug, Clone)]
pub enum AnyData {
    Bool(bool),
    CID(CID),
    String(String),
    U64(u64),
    I64(i64),
    F64(f64),
    AU64(Arc<AtomicU64>),
    AI64(Arc<AtomicI64>),
    Addr(net::Addr),           //store raddr
    User(Box<dyn user::User>), //store authed user
}
impl AnyData {
    pub fn get_type_str(&self) -> &'static str {
        match self {
            AnyData::Bool(_) => "bool",
            AnyData::CID(_) => "cid",
            AnyData::String(_) => "str",
            AnyData::U64(_) => "u",
            AnyData::I64(_) => "i",
            AnyData::F64(_) => "f",
            AnyData::AU64(_) => "au",
            AnyData::AI64(_) => "ai",
            AnyData::Addr(_) => "a",
            AnyData::User(_) => "user",
        }
    }
}

/// 一个 Mapper 实际生成的数据可能是一个单一的数据，也可能是一个数组
#[derive(Debug, Clone)]
pub enum VecAnyData {
    Data(AnyData),
    Vec(Vec<AnyData>),
}

/// Mapper 的 maps 返回的 MapResult 中的实际类型
pub type OptVecData = Option<VecAnyData>;

/// the parameter for Mapper's maps method
#[derive(Default, TypedBuilder)]
pub struct MapParams {
    ///target_addr
    #[builder(default, setter(strip_option))]
    pub a: Option<net::Addr>,

    ///pre_read_buf
    #[builder(default, setter(strip_option))]
    pub b: Option<BytesMut>,

    ///base conn
    #[builder(default)]
    pub c: Stream,

    #[builder(default)]
    pub d: Vec<OptVecData>,

    /// if Stream is a Generator, shutdown_rx should be provided.
    /// it will stop generating if shutdown_rx got msg.
    #[builder(default, setter(strip_option))]
    pub shutdown_rx: Option<oneshot::Receiver<()>>,
}

impl MapParams {
    pub fn new(c: net::Conn) -> Self {
        MapParams::builder().c(Stream::Conn(c)).build()
    }

    pub fn newc(c: net::Conn) -> MapParamsBuilder<((), (), (Stream,), (), ())> {
        MapParams::builder().c(Stream::Conn(c))
    }

    pub fn ca(c: net::Conn, target_addr: net::Addr) -> Self {
        MapParams::newc(c).a(target_addr).build()
    }

    pub fn to_result(self) -> MapResult {
        let rb = MapResult::builder().a(self.a).b(self.b).c(self.c);

        // match self.d {
        //     Some(d) => rb.d(d).build(),
        //     None => rb.build(),
        // }
        if self.d.is_empty() {
            rb.build()
        } else {
            let oolast = self.d.last();
            match oolast {
                Some(olast) => match olast {
                    Some(last) => rb.d(last.clone()).build(),
                    None => rb.build(),
                },
                None => rb.build(),
            }
            //b.d(self.d.last().clone()).build()
        }
    }
}

/// Mapper::maps  return type
///
#[derive(TypedBuilder, Default)]
pub struct MapResult {
    #[builder(default)]
    pub a: Option<net::Addr>, //target_addr

    #[builder(default)]
    pub b: Option<BytesMut>, //pre read buf

    #[builder(default)]
    pub c: Stream,

    #[builder(default, setter(strip_option))]
    pub d: Option<VecAnyData>,

    #[builder(default, setter(strip_option, into))]
    pub e: Option<anyhow::Error>,

    /// 有值代表产生了与之前不同的 cid
    #[builder(default, setter(strip_option))]
    pub new_id: Option<CID>,
}

//some helper initializers
impl MapResult {
    pub fn c(c: net::Conn) -> Self {
        MapResult::builder().c(Stream::c(c)).build()
    }

    pub fn newc(c: net::Conn) -> MapResultBuilder<((), (), (Stream,), (), (), ())> {
        MapResult::builder().c(Stream::c(c))
    }
    pub fn newu(u: AddrConn) -> MapResultBuilder<((), (), (Stream,), (), (), ())> {
        MapResult::builder().c(Stream::u(u))
    }

    pub fn cb(c: net::Conn, b: Option<BytesMut>) -> Self {
        MapResult::newc(c).b(b).build()
    }

    pub fn err_str(estr: &str) -> Self {
        MapResult::builder().e(anyhow!("{}", estr)).build()
    }

    pub fn from_e<E: Into<anyhow::Error>>(e: E) -> Self {
        MapResult::builder().e(e).build()
    }

    pub fn from_result(e: anyhow::Result<MapResult>) -> Self {
        match e {
            Ok(v) => v,
            Err(e) => MapResult::from_e(e),
        }
    }

    pub fn ebc(e: anyhow::Error, b: BytesMut, c: net::Conn) -> Self {
        MapResult::newc(c).e(e).b(buf_to_ob(b)).build()
    }

    pub fn buf_err(b: BytesMut, e: anyhow::Error) -> Self {
        MapResult::builder().e(e).b(buf_to_ob(b)).build()
    }
    pub fn buf_err_str(buf: BytesMut, estr: &str) -> Self {
        MapResult::buf_err(buf, anyhow!("{}", estr))
    }
}

/// indicate the meaning of what the Mapper is really doing
///
/// A proxy would have 2 behaviors in general:
///
/// 1. "encode" the target addr into the stream
/// 2. "decode" the target addr from the stream
///
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyBehavior {
    #[default]
    UNSPECIFIED,

    /// outbound's general behabior
    ENCODE,

    /// inbound's general behabior
    DECODE,
}

/// Mapper: Stream Mapping Function,
///
/// Generally maps just do a handshake in the old Stream, then perhaps forms a/some new Stream
///
/// After encode/decode data in the new Stream,it will be passed to next Mapper
///
#[async_trait]
pub trait Mapper: crate::Name + Debug {
    /// Mapper 在代理逻辑上分 DECODE 和 ENCODE 两种
    ///
    ///   由 behavior 区分。
    ///
    /// # DECODE
    ///
    ///  DECODE 试图生产 MapResult.a 和 MapResult.b ,
    ///
    /// 一旦某一层中获得了 target_addr, 就要继续将它传到下一层。参阅累加器部分。
    ///
    /// 返回值的 MapResult.d 用于 获取关于该层连接的额外信息
    ///
    /// 因为客户端有可能发来除握手数据以外的用户数据(earlydata), 所以返回值里有 MapResult.b,
    /// 其不为None时, 下一级的 maps 就要将其作为 params.b 调用
    ///
    /// 注：切换底层连接是有的协议中会发生的情况, 比如先用 tcp 握手, 之后采用udp; 或者
    /// 先用tcp1 握手, 再换一个端口得到新的tcp2, 用新的连接去传输数据
    ///
    /// 一旦切换连接, 则原连接将不再被 ruci::relay 包控制关闭, 该 Mapper 自行处理.
    ///
    /// 这里不用 Result<...> 的形式, 是因为 在有错误的同时也可能返回一些有用的数据, 比如用于路由,回落等
    ///
    /// # ENCODE
    ///
    ///   是 out client 的 mapper, 从拨号基本连接开始,
    ///  以 targetAddr (不是direct时就不是拔号的那个地址) 为参数创建新层
    ///
    /// 与 DECODE 相比, ENCODE 试图消耗 params.a 和 params.b
    ///
    /// 如果传入的 params.a 不为空, 且 该 层的 maps 将 其消耗掉了, 则返回的 MapResult.a 为None;
    /// 如果没消耗掉, 或是仅对 params.a 做了修改, 则 返回的 MapResult.a 不为 None.
    ///
    async fn maps(&self, cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult;
}

pub trait ToMapper {
    fn to_mapper(&self) -> MapperBox;
}

//令 Mapper 实现 Send + Sync, 否则异步/多线程报错
pub trait MapperSync: MapperExt + Send + Sync {}
impl<T: MapperExt + Send + Sync> MapperSync for T {}

pub type MapperBox = Box<dyn MapperSync>;

/// Some helper fields.
#[derive(Default, Clone, Debug)]

pub struct MapperExtFields {
    pub is_tail_of_chain: bool,
    pub chain_tag: String,
    pub fixed_target_addr: Option<net::Addr>,
    pub pre_defined_early_data: Option<bytes::BytesMut>,
}

/// Some helper method.
///
/// ```plaintext
/// use macro_mapper::*;
/// #[mapper_ext_fields]
/// #[derive(MapperExt)]
///
/// or #[derive(NoMapperExt)]
/// ```
/// to auto impl MapperExt that doesn't do anything.
///
/// See crates/macro_mapper.
///
pub trait MapperExt: Mapper {
    fn get_ext_fields(&self) -> Option<&MapperExtFields>;
    fn set_ext_fields(&mut self, fs: Option<MapperExtFields>);

    fn get_ext_fields_clone_or_default(&self) -> MapperExtFields {
        if let Some(ef) = self.get_ext_fields() {
            ef.clone()
        } else {
            MapperExtFields::default()
        }
    }

    fn set_chain_tag(&mut self, tag: &str) {
        let mut efc = self.get_ext_fields_clone_or_default();

        efc.chain_tag = tag.to_string();
        self.set_ext_fields(Some(efc));
    }

    fn set_is_tail_of_chain(&mut self, is: bool) {
        let mut efc = self.get_ext_fields_clone_or_default();

        efc.is_tail_of_chain = is;
        self.set_ext_fields(Some(efc));
    }

    fn set_configured_target_addr(&mut self, a: Option<net::Addr>) {
        let mut efc = self.get_ext_fields_clone_or_default();

        efc.fixed_target_addr = a;
        self.set_ext_fields(Some(efc));
    }

    fn set_pre_defined_early_data(&mut self, ed: Option<BytesMut>) {
        let mut efc = self.get_ext_fields_clone_or_default();

        efc.pre_defined_early_data = ed;
        self.set_ext_fields(Some(efc));
    }

    fn get_chain_tag(&self) -> &str {
        if let Some(ef) = self.get_ext_fields() {
            return &ef.chain_tag;
        }
        ""
    }

    /// will clone the data
    fn get_pre_defined_early_data(&self) -> Option<BytesMut> {
        if let Some(ef) = self.get_ext_fields() {
            return ef.pre_defined_early_data.clone();
        }
        None
    }
    fn is_tail_of_chain(&self) -> bool {
        if let Some(ef) = self.get_ext_fields() {
            return ef.is_tail_of_chain;
        }
        false
    }
    fn configured_target_addr(&self) -> Option<&net::Addr> {
        if let Some(ef) = self.get_ext_fields() {
            return ef.fixed_target_addr.as_ref();
        }
        None
    }
}
