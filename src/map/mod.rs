/*!
module map defines some important traits for proxy

几个关键部分: AnyData, MapParams, MapResult, Mapper, 和 acc 模块

ruci 包中实现 Mapper 的模块有: math, counter,stdio, network, socks5,http, socks5http, trojan,  tls

ruci 将任意代理行为分割成若干个不可再分的
流映射函数, function map(stream1, args...)-> (stream2, useful_data...)


流映射函数 的提供者 在本包中被命名为 "Mapper", 映射的行为叫 "maps"

在本包中， 映射 不常使用，而经常使用 “加法” 来指代 映射。以“累加”来指代迭代映射。
按顺序执行若干映射函数 的迭代行为 被ruci称为“累加”, 执行者被称为 “累加器”

之所以叫加法，是因为代理的映射只会增加信息（熵），不会减少信息

按代理的方向, 逻辑上分 InAdder 和 OutAdder 两种, 以 maps 方法的 behavior 参数加以区分.


一个完整的代理配置 是 【若干 映射函数 的集合】其在 rucimp 子项目中有定义。


*/

pub mod counter;
pub mod http;
/// math 中有一些基本数学运算的 adder
pub mod math;
pub mod network;
pub mod socks5;
pub mod socks5http;
pub mod stdio;
pub mod tls;
pub mod trojan;

pub mod acc;

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
use tokio::{net::TcpStream, sync::oneshot};
use typed_builder::TypedBuilder;

use std::{fmt::Debug, sync::Arc};

use self::addr_conn::AddrConn;

/// 如果新连接不是udp, 则内含新连接
pub enum NewConnection {
    TcpConnection(TcpStream),
    #[cfg(unix)]
    UnixConnection(tokio::net::UnixStream),
    UdpConnection,
}

pub struct NewConnectionOptData {
    pub new_connection: NewConnection,
    pub data: OptData,
}

#[derive(Debug)]
pub enum AnyData {
    A(AnyArc),
    B(AnyBox),
    Addr(net::Addr),           //store raddr
    User(Box<dyn user::User>), //store authed user
}

pub type OptData = Option<AnyData>;

pub struct InputData {
    pub calculated_data: OptData, //由上层计算得到的数据
    pub hyperparameter: OptData,  // 超参数, 即不上层计算决定的数据
}

/// map方法的参数
#[derive(Default, TypedBuilder)]
pub struct MapParams {
    ///base conn
    #[builder(default)]
    pub c: Stream,

    ///target_addr
    #[builder(default, setter(strip_option))]
    pub a: Option<net::Addr>,

    ///pre_read_buf
    #[builder(default, setter(strip_option))]
    pub b: Option<BytesMut>,

    #[builder(default, setter(strip_option))]
    pub d: Option<InputData>,

    /// if Stream is a Generator, shutdown_rx should be provided.
    /// it will stop generating if shutdown_rx got msg.
    #[builder(default, setter(strip_option))]
    pub shutdown_rx: Option<oneshot::Receiver<()>>,
}

impl MapParams {
    pub fn new(c: net::Conn) -> Self {
        MapParams::builder().c(Stream::TCP(c)).build()
    }

    pub fn newc(c: net::Conn) -> MapParamsBuilder<((Stream,), (), (), (), ())> {
        MapParams::builder().c(Stream::TCP(c))
    }

    pub fn ca(c: net::Conn, target_addr: net::Addr) -> Self {
        MapParams::newc(c).a(target_addr).build()
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

    ///extra data, 如果d为 AnyData::B, 则只能被外部调用;, 如果
    /// d为 AnyData::A, 可其可以作为 下一层的 InputData
    #[builder(default, setter(strip_option))]
    pub d: Option<AnyData>,

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

    pub fn abcod(a: net::Addr, b: BytesMut, c: net::Conn, d: Option<AnyData>) -> Self {
        let builder = MapResult::newc(c).a(Some(a)).b(Some(b));
        match d {
            Some(d) => builder.d(d).build(),
            None => builder.build(),
        }
    }
}

/// indicate the meaning of what the Mapper is really doing
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
/// Generally maps just do a handshake in the old Stream, then forms a new Stream
///
/// After encode/decode data in the new Stream,it will be passed to next Mapper
///
#[async_trait]
pub trait Mapper: crate::Name + Debug {
    /// Mapper 在代理逻辑上分 in 和 out 两种
    ///
    /// InAdder 与 OutAdder 由 behavior 区分。
    ///
    /// # InAdder
    ///
    /// 可选地返回 解析出的“目标地址”。一般只在InAdder最后一级产生。
    /// 且InAdder试图生产出 target_addr 和 pre_read_data
    /// 一旦某一层中获得了 target_addr, 就要继续将它传到下一层。参阅累加器部分。
    ///
    ///
    /// 返回值的 MapResult.d: OptData 用于 获取关于该层连接的额外信息, 一般情况为None即可
    ///
    /// 因为客户端有可能发来除握手数据以外的用户数据(earlydata), 所以返回值里有 Option<BytesMut>,
    /// 其不为None时, 下一级的 maps 就要将其作为 pre_read_buf 调用
    ///
    ///
    ///
    /// 约定：如果一个代理在代理时切换了内部底层连接, 其返回的 extra_data 需为一个
    /// NewConnectionOptData ,  这样 ruci::relay 包才能对其进行识别并处理
    ///
    /// 如果其不是 NewConnectionOptData ,  则不能使用 ruci::relay 作转发逻辑
    ///
    /// 注：切换底层连接是有的协议中会发生的情况, 比如先用 tcp 握手, 之后采用udp; 或者
    /// 先用tcp1 握手, 再换一个端口得到新的tcp2, 用新的连接去传输数据
    ///
    /// 一旦切换连接, 则原连接将不再被 ruci::relay 包控制关闭, 其关闭将由InAdder自行处理.
    /// 这种情况下, 如果 socks5 支持 udp associate, 则 socks5必须为 代理链的最终端。此时base会被关闭, 返回的 AddResult.c 应为 None
    ///
    /// 这里不用 Result<...> 的形式, 是因为 在有错误的同时也可能返回一些有用的数据, 比如用于路由,回落等
    ///
    /// # OutAdder
    ///
    /// OutAdder 是 out client 的 mapper, 从拨号基本连接开始,
    ///  以 targetAddr (不是direct时就不是拔号的那个地址) 为参数创建新层
    ///
    /// 与InAdder 相比, InAdder是试图生产 target_addr 和 earlydata 的机器, 而OutAdder 就是 试图消耗 target_addr 和 earlydata 的机器
    ///
    /// 如果传入的 target_addr不为空, 且 该 层的add 将 其消耗掉了, 则返回的 Option<net::Addr> 为None;
    /// 如果没消耗掉, 或是仅对 target_addr 做了修改, 则 返回的 Option<net::Addr> 不为 None.
    ///
    /// 与 InAdder 一样, 它返回一个可选的额外数据  OptData
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
/// to auto impl MapperExt
///
/// See crates/macro_mapper.
///
pub trait MapperExt: Mapper {
    fn get_ext_fields(&self) -> Option<&MapperExtFields>;
    fn set_ext_fields(&mut self, fs: Option<MapperExtFields>);

    fn set_chain_tag(&mut self, tag: &str) {
        let mut efc = if let Some(ef) = self.get_ext_fields() {
            ef.clone()
        } else {
            MapperExtFields::default()
        };

        efc.chain_tag = tag.to_string();
        self.set_ext_fields(Some(efc));
    }

    fn set_is_tail_of_chain(&mut self, is: bool) {
        let mut efc = if let Some(ef) = self.get_ext_fields() {
            ef.clone()
        } else {
            MapperExtFields::default()
        };

        efc.is_tail_of_chain = is;
        self.set_ext_fields(Some(efc));
    }

    fn set_configured_target_addr(&mut self, a: Option<net::Addr>) {
        let mut efc = if let Some(ef) = self.get_ext_fields() {
            ef.clone()
        } else {
            MapperExtFields::default()
        };

        efc.fixed_target_addr = a;
        self.set_ext_fields(Some(efc));
    }

    fn get_chain_tag(&self) -> &str {
        if let Some(ef) = self.get_ext_fields() {
            return &ef.chain_tag;
        }
        ""
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
