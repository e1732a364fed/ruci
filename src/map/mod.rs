/*!
module map defines some important traits for proxy

几个关键部分: AnyData, MapParams, MapResult, Mapper,accumulate , accumulate_from_start

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

#[cfg(test)]
mod test;

use crate::{
    net::{
        self, addr_conn::AddrConn, new_ordered_cid, new_rand_cid, CIDChain, Stream,
        TransmissionInfo, CID,
    },
    AnyArc, AnyBox,
};

use async_trait::async_trait;
use bytes::BytesMut;
use dyn_clone::DynClone;
use log::{info, log_enabled, warn};
use tokio::{net::TcpStream, sync::oneshot};

use std::{fmt::Debug, io, sync::Arc};

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
    Addr(net::Addr),
}

pub type OptData = Option<AnyData>;

pub struct InputData {
    pub calculated_data: OptData, //由上层计算得到的数据
    pub hyperparameter: OptData,  // 超参数, 即不上层计算决定的数据
}

/// map方法的参数
pub struct MapParams {
    ///base conn
    pub c: Stream,

    ///target_addr
    pub a: Option<net::Addr>,

    ///pre_read_buf
    pub b: Option<BytesMut>,

    pub d: Option<InputData>,

    /// if Stream is a Generator, shutdown_rx should be provided.
    /// it will stop generating if shutdown_rx got msg.
    pub shutdown_rx: Option<oneshot::Receiver<()>>,
}

impl MapParams {
    pub fn new(c: net::Conn) -> Self {
        MapParams {
            c: Stream::TCP(c),
            a: None,
            b: None,
            d: None,
            shutdown_rx: None,
        }
    }

    pub fn ca(c: net::Conn, target_addr: net::Addr) -> Self {
        MapParams {
            c: Stream::TCP(c),
            a: Some(target_addr),
            b: None,
            d: None,
            shutdown_rx: None,
        }
    }
}

/// add 方法的返回值
#[derive(Default)]
pub struct MapResult {
    pub a: Option<net::Addr>, //target_addr
    pub b: Option<BytesMut>,  //pre read buf
    pub c: Stream,

    ///extra data, 如果d为 AnyData::B, 则只能被外部调用;, 如果
    /// d为 AnyData::A, 可其可以作为 下一层的 InputData
    pub d: OptData,
    pub e: Option<io::Error>,

    /// 有值代表产生了与之前不同的 cid
    pub new_id: Option<CID>,
}

//some helper initializers
impl MapResult {
    pub fn ac(a: net::Addr, c: net::Conn) -> Self {
        MapResult {
            a: Some(a),
            b: None,
            c: Stream::TCP(c),
            d: None,
            e: None,
            new_id: None,
        }
    }
    pub fn oac(a: Option<net::Addr>, c: net::Conn) -> Self {
        MapResult {
            a,
            b: None,
            c: Stream::TCP(c),
            d: None,
            e: None,
            new_id: None,
        }
    }

    /// will set b to None if b.len() == 0
    pub fn abc(a: net::Addr, b: BytesMut, c: net::Conn) -> Self {
        MapResult {
            a: Some(a),
            b: if b.is_empty() { None } else { Some(b) },
            c: Stream::TCP(c),
            d: None,
            e: None,
            new_id: None,
        }
    }

    pub fn abcod(a: net::Addr, b: BytesMut, c: net::Conn, d: Option<AnyData>) -> Self {
        MapResult {
            a: Some(a),
            b: if b.is_empty() { None } else { Some(b) },
            c: Stream::TCP(c),
            d,
            e: None,
            new_id: None,
        }
    }

    pub fn oabc(a: Option<net::Addr>, b: Option<BytesMut>, c: net::Conn) -> Self {
        MapResult {
            a,
            b,
            c: Stream::TCP(c),
            d: None,
            e: None,
            new_id: None,
        }
    }

    pub fn obc(b: Option<BytesMut>, c: net::Conn) -> Self {
        MapResult {
            a: None,
            b,
            c: Stream::TCP(c),
            d: None,
            e: None,
            new_id: None,
        }
    }

    pub fn udp_abc(a: net::Addr, b: BytesMut, c: AddrConn) -> Self {
        MapResult {
            a: Some(a),
            b: if b.is_empty() { None } else { Some(b) },
            c: Stream::UDP(c),
            d: None,
            e: None,
            new_id: None,
        }
    }

    pub fn c(c: net::Conn) -> Self {
        MapResult {
            a: None,
            b: None,
            c: Stream::TCP(c),
            d: None,
            e: None,
            new_id: None,
        }
    }

    pub fn s(s: net::Stream) -> Self {
        MapResult {
            a: None,
            b: None,
            c: s,
            d: None,
            e: None,
            new_id: None,
        }
    }

    //Generator
    pub fn gs(gs: tokio::sync::mpsc::Receiver<MapResult>, cid: CID) -> Self {
        MapResult {
            a: None,
            b: None,
            c: Stream::Generator(gs),
            d: None,
            e: None,
            new_id: Some(cid),
        }
    }

    pub fn from_err(e: io::Error) -> Self {
        MapResult {
            a: None,
            b: None,
            c: Stream::None,
            d: None,
            e: Some(e),
            new_id: None,
        }
    }

    pub fn err_str(estr: &str) -> Self {
        MapResult::from_err(io::Error::other(estr))
    }

    pub fn from_result(e: io::Result<MapResult>) -> Self {
        match e {
            Ok(v) => v,
            Err(e) => MapResult::from_err(e),
        }
    }

    pub fn ebc(e: io::Error, buf: BytesMut, c: net::Conn) -> Self {
        MapResult {
            a: None,
            b: Some(buf),
            c: Stream::TCP(c),
            d: None,
            e: Some(e),
            new_id: None,
        }
    }
    pub fn buf_err(buf: BytesMut, e: io::Error) -> Self {
        MapResult {
            a: None,
            b: Some(buf),
            c: Stream::None,
            d: None,
            e: Some(e),
            new_id: None,
        }
    }
    pub fn buf_err_str(buf: BytesMut, estr: &str) -> Self {
        MapResult::buf_err(buf, io::Error::other(estr))
    }
}

/// 指示某 Mapping 行为的含义
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyBehavior {
    #[default]
    UNSPECIFIED,

    /// out client 的行为
    ENCODE,

    ///in server 的行为
    DECODE,
}

/// Mapper 流映射函数, 在一个Conn 的基础上添加新read/write层, 形成一个新Conn
///
/// 且InAdder试图生产出 target_addr 和 pre_read_data
///
/// 一般来说 maps 方法就是执行一个新层中的握手, 之后得到一个新Conn;
/// 在 新Conn中 对数据进行加/解密后, pass to next layer Conn
///
/// 一旦某一层中获得了 target_addr, 就要继续将它传到下一层。参阅累加器部分。
///
/// 因为客户端有可能发来除握手数据以外的用户数据(earlydata), 所以返回值里有 Option<BytesMut>,
/// 其不为None时, 下一级的 add就要将其作为 pre_read_buf 调用
///
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
    ///
    /// 返回值的 MapResult.d: OptData 用于 获取关于该层连接的额外信息, 一般情况为None即可
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

/// 一些辅助方法. See crates/macro_mapper.
/// ```plaintext
/// use macro_mapper::*;
/// #[common_mapper_field]
/// #[derive(CommonMapperExt)]
///
/// 或 #[derive(DefaultMapperExt)]
/// ```
/// 来自动添加实现
pub trait MapperExt: Mapper {
    fn set_configured_target_addr(&mut self, _a: Option<net::Addr>);
    fn set_is_tail_of_chain(&mut self, _is: bool);

    fn configured_target_addr(&self) -> Option<net::Addr>;
    fn is_tail_of_chain(&self) -> bool;

    fn set_chain_tag(&mut self, tag: &str);

    fn get_chain_tag(&self) -> &str;
}

//令 Mapper 实现 Send + Sync, 否则异步/多线程报错
pub trait MapperSync: MapperExt + Send + Sync + DynClone {}
impl<T: MapperExt + Send + Sync + DynClone> MapperSync for T {}
dyn_clone::clone_trait_object!(MapperSync);

pub type MapperBox = Box<dyn MapperSync>; //必须用Box,不能直接是 Arc

pub trait MIter: Iterator<Item = &'static MapperBox> + DynClone + Send + Sync + Debug {}
impl<T: Iterator<Item = &'static MapperBox> + DynClone + Send + Sync + Debug> MIter for T {}
dyn_clone::clone_trait_object!(MIter);

pub type MIterBox = Box<dyn MIter>;

//MIterBox 才是传统意义上的 一条代理链. 一个 MapperBox 只是链中的一环.
//MIterBox 有静态的生命周期，因此其内存必须由程序手动管理

pub struct AccumulateResult {
    pub a: Option<net::Addr>,
    pub b: Option<BytesMut>,
    pub c: Stream,
    pub d: Vec<OptData>,
    pub e: Option<io::Error>,

    /// 代表 迭代完成后，最终的 cid
    pub id: Option<CID>,

    pub chain_tag: String,
    /// 累加后剩余的iter(用于一次加法后产生了 Generator 的情况)
    pub left_mappers_iter: MIterBox,
}

impl Debug for AccumulateResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccumulateResult")
            .field("a", &self.a)
            .field("b", &self.b)
            .field("c", &self.c)
            .field("d", &self.d)
            .field("e", &self.e)
            .field("id", &self.id)
            .field("tag", &self.chain_tag)
            //.field("left_mappers_iter count", &self.left_mappers_iter.)
            .finish()
    }
}

///  accumulate 是一个作用很强的函数,是 mappers 的累加器
///
/// cid 为 跟踪 该连接的 标识
/// 返回的元组包含新的 Conn 和 可能的目标地址
///
/// decode: 用途： 从listen得到的tcp开始, 一层一层往上加, 直到加到能解析出代理目标地址为止
///
/// 一般 【中同层是返回的 target_addr都是None, 只有最后一层会返回出目标地址, 即,
///只有代理层会有目标地址】
///
/// 注意, 考虑在两个累加结果的Conn之间拷贝, 若用 ruci::net::cp 拷贝并给出 TransmissionInfo,
/// 则它统计出的流量为 未经加密的原始流量, 实际流量一般会比原始流量大。要想用
/// ruci::net::cp 统计真实流量, 只能有一种情况, 那就是 tcp到tcp的直接拷贝,
/// 不使用累加器。
///
/// 一种统计正确流量的办法是, 将 Tcp连接包装一层专门记录流量的层, 见 counter 模块
///
/// accumulate 只适用于 不含 Stream::Generator 的情况,
///
/// 结果中 Stream为 None 或 一个 Stream::Generator , 或e不为None时, 将退出累加
///
/// 能生成 Stream::Generator 说明其 behavior 为 DECODE
///
pub async fn accumulate(
    cid: CID,
    behavior: ProxyBehavior,
    initial_state: MapResult,
    mut mappers: MIterBox,
) -> AccumulateResult {
    let mut last_r: MapResult = initial_state;

    let mut calculated_output_vec = Vec::new();

    let mut tag: String = String::new();

    for adder in mappers.by_ref() {
        let input_data = InputData {
            calculated_data: calculated_output_vec
                .last()
                .and_then(|x: &Option<AnyData>| {
                    x.as_ref().and_then(|y| match y {
                        AnyData::A(a) => Some(AnyData::A(a.clone())),
                        _ => None,
                    })
                }),
            hyperparameter: None,
        };
        let input_data =
            if input_data.calculated_data.is_none() && input_data.calculated_data.is_none() {
                None
            } else {
                Some(input_data)
            };
        last_r = adder
            .maps(
                match last_r.new_id {
                    Some(id) => id,
                    None => cid.clone(),
                },
                behavior,
                MapParams {
                    c: last_r.c,
                    a: last_r.a,
                    b: last_r.b,
                    d: input_data,
                    shutdown_rx: None,
                },
            )
            .await;

        if tag == "" {
            let ct = adder.get_chain_tag();

            if ct != "" {
                tag = ct.to_string();
            }
        }

        calculated_output_vec.push(last_r.d);

        if let Stream::None = last_r.c {
            break;
        }
        if last_r.e.is_some() {
            break;
        }
        if let Stream::Generator(_) = last_r.c {
            break;
        }
    }

    AccumulateResult {
        a: last_r.a,
        b: last_r.b,
        c: last_r.c,
        d: calculated_output_vec,
        e: last_r.e,
        id: if last_r.new_id.is_some() {
            last_r.new_id
        } else {
            Some(cid)
        },
        left_mappers_iter: mappers,
        chain_tag: tag,
    }
}

/// blocking.
/// 先调用第一个 mapper 生成 流, 然后调用 in_iter_accumulate_forever
pub async fn accumulate_from_start(
    tx: tokio::sync::mpsc::Sender<AccumulateResult>,
    shutdown_rx: oneshot::Receiver<()>,

    mut inmappers: MIterBox,
    oti: Option<Arc<TransmissionInfo>>,
) {
    let first = inmappers.next().expect("first inmapper");
    let r = first
        .maps(
            CID::default(),
            ProxyBehavior::DECODE,
            MapParams {
                c: Stream::None,
                a: None,
                b: None,
                d: None,
                shutdown_rx: Some(shutdown_rx),
            },
        )
        .await;
    if let Some(e) = r.e {
        warn!("accumulate_from_start, returned by e, {}", e);
        return;
    }

    if let Stream::Generator(rx) = r.c {
        in_iter_accumulate_forever(CID::default(), rx, tx, inmappers, oti).await;
    } else {
        warn!(
            "accumulate_from_start: not a stream generator, will accumulate directly. {}",
            r.c
        );

        tokio::spawn(async move {
            let r = accumulate(
                CID::new_by_opti(oti),
                ProxyBehavior::DECODE,
                MapResult {
                    a: r.a,
                    b: r.b,
                    c: r.c,
                    d: r.d,
                    e: None,
                    new_id: None,
                },
                inmappers,
            )
            .await;
            let _ = tx.send(r).await;
        });
    }
}

/// blocking.block until rx got closed.
/// 用于 已知一个初始点为 Stream::Generator (rx), 向其所有子连接进行accumulate,
/// 直到遇到结果中 Stream为 None 或 一个 Stream::Generator, 或e不为None
///
/// 因为要spawn, 所以对 Iter 的类型提了比 accumulate更高的要求, 加了
/// Clone + Send + 'static
///
/// 将每一条子连接的accumulate 结果 用 tx 发送出去;
///
/// 如果子连接又是一个 Stream::Generator, 则不会继续调用 自己 进行递归
/// 因为 会报错 cycle detected when computing type;
///
/// 这里只能返回给调用者去处理
pub async fn in_iter_accumulate_forever(
    cid: CID,
    mut rx: tokio::sync::mpsc::Receiver<MapResult>, //Stream::Generator
    tx: tokio::sync::mpsc::Sender<AccumulateResult>,
    inmappers: MIterBox,
    oti: Option<Arc<TransmissionInfo>>,
) {
    loop {
        let opt_stream = rx.recv().await;

        let new_stream_info = match opt_stream {
            Some(s) => s,
            None => break,
        };

        let mc = inmappers.clone();
        let txc = tx.clone();
        let oti = oti.clone();

        let cid = cid.clone();
        let cid = match cid {
            CID::Unit(u) => match oti.as_ref() {
                Some(ti) => {
                    if u == 0 {
                        CID::new_ordered(&ti.last_connection_id)
                    } else {
                        CID::Chain(CIDChain {
                            id_list: vec![u, new_ordered_cid(&ti.last_connection_id)],
                        })
                    }
                }
                None => CID::new(),
            },
            CID::Chain(mut c) => match oti.as_ref() {
                Some(ti) => {
                    c.id_list.push(new_ordered_cid(&ti.last_connection_id));
                    CID::Chain(c)
                }
                None => {
                    c.id_list.push(new_rand_cid());
                    CID::Chain(c)
                }
            },
        };

        if log_enabled!(log::Level::Info) {
            info!("{cid}, new accepted stream");
        }

        tokio::spawn(async move {
            let r = accumulate(cid, ProxyBehavior::DECODE, new_stream_info, mc).await;
            let _ = txc.send(r).await;
        });
    }
}
