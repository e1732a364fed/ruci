/*!
module proxy define some important traits for proxy

几个关键部分: State, Stream, Mapper

ruci 包中实现 Mapper 的模块有: math, counter, tls, socks5, trojan

ruci 将任意代理行为分割成若干个不可再分的
流映射函数, function map(stream1, args...)-> (stream2, useful_data...)


流映射函数 的提供者 在本包中被命名为 "Mapper", 映射的行为叫 "maps"

按代理的方向，逻辑上分 InAdder 和 OutAdder 两种，以 maps 方法的 behavior 参数加以区分.

按顺序执行若干映射函数 的迭代行为 被ruci称为“累加”, 执行者被称为 “累加器”

一个完整的代理配置 是 【若干 映射函数 的集合】其在 rucimp 子项目中有定义。


*/

pub mod counter;

/// math 中有一些基本数学运算的 adder
pub mod math;
pub mod network;
pub mod socks5;
pub mod tls;
pub mod trojan;

#[cfg(test)]
mod test;

use crate::net::{
    self, addr_conn::AddrConn, new_ordered_cid, new_rand_cid, InStreamCID, Stream,
    TransmissionInfo, CID,
};

use async_trait::async_trait;
use bytes::BytesMut;
use dyn_clone::DynClone;
use log::warn;
use tokio::{
    net::TcpStream,
    sync::{oneshot, Mutex},
};

use std::{
    any::Any,
    fmt::{Debug, Display},
    io,
    sync::Arc,
};

/// 描述一条代理连接
pub trait State<T>
where
    T: Display,
{
    fn cid(&self) -> CID;
    fn network(&self) -> &'static str;
    fn ins_name(&self) -> String; // 入口名称
    fn set_ins_name(&mut self, str: String);
    fn outc_name(&self) -> String; // 出口名称
    fn set_outc_name(&mut self, str: String);

    fn cached_in_raddr(&self) -> String; // 进入程序时的 连接 的远端地址
    fn set_cached_in_raddr(&mut self, str: String);
}

/// 实现 State, 其没有 parent
#[derive(Debug, Default, Clone)]
pub struct RootState {
    pub cid: u32, //固定十进制位数的数
    pub network: &'static str,
    pub ins_name: String,        // 入口名称
    pub outc_name: String,       // 出口名称
    pub cached_in_raddr: String, // 进入程序时的 连接 的远端地址
}

impl RootState {
    /// new with random id
    pub fn new(network: &'static str) -> RootState {
        let mut s = RootState::default();
        s.cid = net::new_rand_cid();
        s.network = network;
        s
    }

    /// new with ordered id
    pub fn new_ordered(network: &'static str, lastid: &std::sync::atomic::AtomicU32) -> RootState {
        let li = lastid.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut s = RootState::default();
        s.cid = li + 1;
        s.network = network;
        s
    }
}

impl Display for RootState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.outc_name() == "" {
            write!(
                f,
                "[ cid: {}, {}://{}, listener: {}, ]  ",
                self.cid(),
                self.network(),
                self.cached_in_raddr(),
                self.ins_name()
            )
        } else {
            write!(
                f,
                "[ cid: {}, {}://{}, route from: {}, to: {} ]  ",
                self.cid(),
                self.network(),
                self.cached_in_raddr(),
                self.ins_name(),
                self.outc_name(),
            )
        }
    }
}

impl State<u32> for RootState {
    fn cid(&self) -> CID {
        CID::Unit(self.cid)
    }

    fn network(&self) -> &'static str {
        self.network
    }

    fn ins_name(&self) -> String {
        self.ins_name.clone()
    }

    fn outc_name(&self) -> String {
        self.outc_name.clone()
    }

    fn cached_in_raddr(&self) -> String {
        self.cached_in_raddr.clone()
    }

    fn set_ins_name(&mut self, str: String) {
        self.ins_name = str
    }

    fn set_outc_name(&mut self, str: String) {
        self.outc_name = str
    }

    fn set_cached_in_raddr(&mut self, str: String) {
        self.cached_in_raddr = str
    }
}

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

pub type AnyS = dyn Any + Send; // 加 Send 以支持多线程
pub type AnyBox = Box<AnyS>;
pub type AnyArc = Arc<Mutex<AnyS>>;

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

    pub id: Option<CID>, //有值代表产生了与之前不同的 cid
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
            id: None,
        }
    }

    /// will set b to None if b.len() == 0
    pub fn abc(a: net::Addr, b: BytesMut, c: net::Conn) -> Self {
        MapResult {
            a: Some(a),
            b: if b.len() > 0 { Some(b) } else { None },
            c: Stream::TCP(c),
            d: None,
            e: None,
            id: None,
        }
    }

    pub fn udp_abc(a: net::Addr, b: BytesMut, c: AddrConn) -> Self {
        MapResult {
            a: Some(a),
            b: if b.len() > 0 { Some(b) } else { None },
            c: Stream::UDP(c),
            d: None,
            e: None,
            id: None,
        }
    }

    pub fn c(c: net::Conn) -> Self {
        MapResult {
            a: None,
            b: None,
            c: Stream::TCP(c),
            d: None,
            e: None,
            id: None,
        }
    }

    pub fn s(s: net::Stream) -> Self {
        MapResult {
            a: None,
            b: None,
            c: s,
            d: None,
            e: None,
            id: None,
        }
    }

    //Generator
    pub fn gs(gs: tokio::sync::mpsc::Receiver<Stream>, cid: CID) -> Self {
        MapResult {
            a: None,
            b: None,
            c: Stream::Generator(gs),
            d: None,
            e: None,
            id: Some(cid),
        }
    }

    pub fn from_err(e: io::Error) -> Self {
        MapResult {
            a: None,
            b: None,
            c: Stream::None,
            d: None,
            e: Some(e),
            id: None,
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
    pub fn buf_err(buf: BytesMut, e: io::Error) -> Self {
        MapResult {
            a: None,
            b: Some(buf),
            c: Stream::None,
            d: None,
            e: Some(e),
            id: None,
        }
    }
    pub fn buf_err_str(buf: BytesMut, estr: &str) -> Self {
        MapResult::buf_err(buf, io::Error::other(estr))
    }
}

/// 指示某 Mapping 行为的含义
#[derive(Default, Debug, Clone, Copy)]
pub enum ProxyBehavior {
    #[default]
    UNSPECIFIED,

    /// out client 的行为
    ENCODE,

    ///in server 的行为
    DECODE,
}

/// Mapper 流映射函数, 在一个Conn 的基础上添加新read/write层，形成一个新Conn
///
/// 且InAdder试图生产出 target_addr 和 pre_read_data
///
/// 一般来说add方法就是执行一个新层中的握手，之后得到一个新Conn;
/// 在 新Conn中 对数据进行加/解密后，pass to next layer Conn
///
/// 一旦某一层中获得了 target_addr, 就要继续将它传到下一层。参阅累加器部分。
///
/// 因为客户端有可能发来除握手数据以外的用户数据(earlydata), 所以返回值里有 Option<BytesMut>，
/// 其不为None时，下一级的 add就要将其作为 pre_read_buf 调用
///
///
#[async_trait]
pub trait Mapper: crate::Name + DynClone {
    /// InAdder 与 OutAdder 由 behavior 区分。
    ///
    /// # InAdder
    ///
    /// 可选地返回 解析出的“目标地址”。一般只在InAdder最后一级产生。
    ///
    /// 返回值的 MapResult.d: OptData 用于 获取关于该层连接的额外信息, 一般情况为None即可
    ///
    /// 约定：如果一个代理在代理时切换了内部底层连接，其返回的 extra_data 需为一个
    /// NewConnectionOptData ， 这样 ruci::relay 包才能对其进行识别并处理
    ///
    /// 如果其不是 NewConnectionOptData ， 则不能使用 ruci::relay 作转发逻辑
    ///
    /// 注：切换底层连接是有的协议中会发生的情况，比如先用 tcp 握手，之后采用udp；或者
    /// 先用tcp1 握手，再换一个端口得到新的tcp2, 用新的连接去传输数据
    ///
    /// 一旦切换连接，则原连接将不再被 ruci::relay 包控制关闭, 其关闭将由InAdder自行处理.
    /// 这种情况下，如果 socks5 支持 udp associate，则 socks5必须为 代理链的最终端。此时base会被关闭，返回的 AddResult.c 应为 None
    ///
    /// 这里不用 Result<...> 的形式，是因为 在有错误的同时也可能返回一些有用的数据，比如用于回落等
    ///
    /// # OutAdder
    ///
    /// OutAdder 是 out client 的 adder，从拨号基本连接开始,
    ///  以 targetAddr (不是direct时就不是拔号的那个地址) 为参数创建新层
    ///
    /// 与InAdder 相比，InAdder是试图生产 target_addr 和 earlydata 的机器，而OutAdder 就是 试图消耗 target_addr 和 earlydata 的机器
    ///
    /// 如果传入的 target_addr不为空，且 该 层的add 将 其消耗掉了，则返回的 Option<net::Addr> 为None；
    /// 如果没消耗掉，或是仅对 target_addr 做了修改，则 返回的 Option<net::Addr> 不为 None.
    ///
    /// 与 InAdder 一样，它返回一个可选的额外数据  OptData
    ///
    async fn maps(&self, cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult;

    fn get_target_addr(&self) -> Option<net::Addr> {
        None
    }
}

dyn_clone::clone_trait_object!(MapperSync);

pub trait ToMapper {
    fn to_mapper(&self) -> MapperBox;
}

//令 Mapper 实现 Send + Sync，否则异步/多线程报错
pub trait MapperSync: Mapper + Send + Sync + Debug {}
impl<T: Mapper + Send + Sync + Debug> MapperSync for T {}

pub type MapperBox = Box<dyn MapperSync>; //必须用Box,不能直接是 Arc

pub struct AccumulateResult<'a, IterMapperBoxRef>
where
    IterMapperBoxRef: Iterator<Item = &'a MapperBox>,
{
    pub a: Option<net::Addr>,
    pub b: Option<BytesMut>,
    pub c: Stream,
    pub d: Vec<OptData>,
    pub e: Option<io::Error>,

    pub id: Option<CID>, //有值代表产生了与之前不同的 cid
    pub left_mappers_iter: IterMapperBoxRef,
}

///  accumulate 是一个作用很强的函数,是 mappers 的累加器
///
/// cid 为 跟踪 该连接的 标识
/// 返回的元组包含新的 Conn 和 可能的目标地址
///
/// decode: 用途： 从listen得到的tcp开始，一层一层往上加，直到加到能解析出代理目标地址为止
///
/// 一般 【中同层是返回的 target_addr都是None，只有最后一层会返回出目标地址，即，
///只有代理层会有目标地址】
///
/// 注意，考虑在两个累加结果的Conn之间拷贝，若用 ruci::net::cp 拷贝并给出 TransmissionInfo,
/// 则它统计出的流量为 未经加密的原始流量，实际流量一般会比原始流量大。要想用
/// ruci::net::cp 统计真实流量，只能有一种情况，那就是 tcp到tcp的直接拷贝，
/// 不使用累加器。
///
/// 一种统计正确流量的办法是，将 Tcp连接包装一层专门记录流量的层，见 counter 模块
///
/// extra_data_vec 若不为空，其须与 mappers 提供同数量的元素, 否则
/// 将panic
///
/// accumulate 只适用于 不含 Stream::Generator 的情况,
///
/// 结果中 Stream为 None 或 一个 Stream::Generator ，或e不为None时，将退出累加
///
/// 能生成 Stream::Generator 说明其 behavior 为 DECODE
///
pub async fn accumulate<'a, IterMapperBoxRef>(
    cid: CID,
    behavior: ProxyBehavior,
    initial_state: MapResult,
    mut mappers: IterMapperBoxRef,
) -> AccumulateResult<'a, IterMapperBoxRef>
where
    IterMapperBoxRef: Iterator<Item = &'a MapperBox>,
{
    let mut last_r: MapResult = initial_state;

    let mut calculated_output_vec = Vec::new();

    loop {
        match mappers.next() {
            Some(adder) => {
                let input_data = InputData {
                    calculated_data: match calculated_output_vec.last() {
                        Some(x) => match x {
                            Some(y) => match y {
                                AnyData::A(a) => {
                                    let na = a.clone();
                                    Some(AnyData::A(na))
                                }
                                _ => None,
                            },
                            None => None,
                        },
                        None => None,
                    },
                    // hyperparameter: if let Some(v) = hyperparameter_vec.as_mut() {
                    //     v.next().unwrap()
                    // } else {
                    //     None
                    // },
                    hyperparameter: None,
                };
                let input_data = if input_data.calculated_data.is_none()
                    && input_data.calculated_data.is_none()
                {
                    None
                } else {
                    Some(input_data)
                };
                last_r = adder
                    .maps(
                        match last_r.id {
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
            None => {
                break;
            }
        }
    }

    return AccumulateResult {
        a: last_r.a,
        b: last_r.b,
        c: last_r.c,
        d: calculated_output_vec,
        e: last_r.e,
        id: if last_r.id.is_some() {
            last_r.id
        } else {
            Some(cid)
        },
        left_mappers_iter: mappers,
    };
}

/// 先调用第一个 mapper 生成 流，然后调用 in_iter_accumulate
pub async fn accumulate_from_start<IterMapperBoxRef>(
    tx: tokio::sync::mpsc::Sender<AccumulateResult<'static, IterMapperBoxRef>>,
    shutdown_rx: oneshot::Receiver<()>,

    mut inmappers: IterMapperBoxRef,
    oti: Option<Arc<TransmissionInfo>>,
) where
    IterMapperBoxRef: Iterator<Item = &'static MapperBox> + Clone + Send + 'static,
{
    let first = inmappers.next().unwrap();
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

    if r.e.is_some() {
        warn!("accumulate_from_start, returned by e");
        return;
    }
    if let Stream::Generator(rx) = r.c {
        in_iter_accumulate_forever::<IterMapperBoxRef>(CID::default(), rx, tx, inmappers, oti)
            .await;
    } else {
        warn!("accumulate_from_start, not a stream generator");

        return;
    }
}

/// 用于 已知一个初始点为 Stream::Generator (rx), 向其所有子连接进行accumulate,
/// 直到遇到结果中 Stream为 None 或 一个 Stream::Generator, 或e不为None
///
/// 因为要spawn, 所以对 Iter 的类型提了比 accumulate更高的要求, 加了
/// Clone + Send + 'static
///
/// 将每一条子连接的accumulate 结果 用 tx 发送出去;
/// block until rx got closed.
/// 如果子连接又是一个 Stream::Generator, 则会继续调用 自己 进行递归
pub async fn in_iter_accumulate_forever<IterMapperBoxRef>(
    cid: CID,
    mut rx: tokio::sync::mpsc::Receiver<Stream>, //Stream::Generator
    tx: tokio::sync::mpsc::Sender<AccumulateResult<'static, IterMapperBoxRef>>,
    inmappers: IterMapperBoxRef,
    oti: Option<Arc<TransmissionInfo>>,
) where
    IterMapperBoxRef: Iterator<Item = &'static MapperBox> + Clone + Send + 'static,
{
    loop {
        let opt_stream = rx.recv().await;
        if opt_stream.is_none() {
            break;
        }
        let stream = opt_stream.unwrap();

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
                        CID::Chain(InStreamCID {
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

        tokio::spawn(async move {
            let r = accumulate::<IterMapperBoxRef>(
                cid,
                ProxyBehavior::DECODE,
                MapResult::s(stream),
                mc,
            )
            .await;
            if r.e.is_none() {
                if let Stream::Generator(s_rx) = r.c {
                    let cid = r.id.unwrap();
                    let _ = in_iter_accumulate_forever::<IterMapperBoxRef>(
                        cid,
                        s_rx,
                        txc,
                        r.left_mappers_iter,
                        oti,
                    );

                    return;
                }
            }
            let _ = txc.send(r).await;
        });
    }
}
