/*!
module proxy define some important traits for proxy

几个关键部分：State, Mapper, Accumulator

ruci 包中实现 Mapper 的模块有：math, counter, tls, socks5, trojan

ruci 将任意代理行为分割成若干个不可再分的
流映射函数, function map(stream1, args...)-> (stream2, useful_data...)


流映射函数 的提供者 在本包中被命名为 "Mapper", 映射的行为叫 "maps"

按代理的方向，逻辑上分 InAdder 和 OutAdder 两种，以 maps 方法的 behavior 参数加以区分.

按顺序执行若干映射函数 的迭代行为 被ruci称为“累加”，执行者被称为 “累加器”

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

use crate::net::{self, addr_conn::AddrConn};

use async_trait::async_trait;
use bytes::BytesMut;
use tokio::{net::TcpStream, sync::Mutex};

use std::{
    any::Any,
    fmt::{Debug, Display},
    io,
    sync::Arc,
};

/// 描述一条代理连接
#[derive(Debug, Default, Clone)]
pub struct State {
    pub cid: u32, //固定十进制位数的数
    pub network: &'static str,
    pub ins_name: String,        // 入口名称
    pub outc_name: String,       // 出口名称
    pub cached_in_raddr: String, // 进入程序时的 连接 的远端地址
}

impl Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.outc_name == "" {
            write!(
                f,
                "[ cid: {}, {}://{}, listener: {}, ]  ",
                self.cid, self.network, self.cached_in_raddr, self.ins_name
            )
        } else {
            write!(
                f,
                "[ cid: {}, {}://{}, route from: {}, to: {} ]  ",
                self.cid, self.network, self.cached_in_raddr, self.ins_name, self.outc_name,
            )
        }
    }
}

impl State {
    pub fn new(network: &'static str) -> State {
        use rand::Rng;
        const ID_RANGE_START: u32 = 100_000;

        let mut s = State::default();
        s.cid = rand::thread_rng().gen_range(ID_RANGE_START..=ID_RANGE_START * 10 - 1);
        s.network = network;
        s
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
    calculated_data: OptData, //由上层计算得到的数据
    hyperparameter: OptData,  // 超参数, 即不上层计算决定的数据
}

pub enum Stream {
    TCP(net::Conn), // 传播 tcp / unix domain socket 数据
    UDP(AddrConn),
    None,
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
}

impl MapParams {
    pub fn new(c: net::Conn) -> Self {
        MapParams {
            c: Stream::TCP(c),
            a: None,
            b: None,
            d: None,
        }
    }

    pub fn ca(c: net::Conn, target_addr: net::Addr) -> Self {
        MapParams {
            c: Stream::TCP(c),
            a: Some(target_addr),
            b: None,
            d: None,
        }
    }
}

/// add 方法的返回值
#[derive(Default)]
pub struct MapResult {
    pub a: Option<net::Addr>, //target_addr
    pub b: Option<BytesMut>,  //pre read buf
    pub c: Option<net::Conn>,

    ///extra data, 如果d为 AnyData::B, 则只能被外部调用;, 如果
    /// d为 AnyData::A, 可其可以作为 下一层的 InputData
    pub d: OptData,
    pub e: Option<io::Error>,
}

//some helper initializers
impl MapResult {
    pub fn ac(a: net::Addr, c: net::Conn) -> Self {
        MapResult {
            a: Some(a),
            b: None,
            c: Some(c),
            d: None,
            e: None,
        }
    }

    /// will set b to None if b.len() == 0
    pub fn abc(a: net::Addr, b: BytesMut, c: net::Conn) -> Self {
        MapResult {
            a: Some(a),
            b: if b.len() > 0 { Some(b) } else { None },
            c: Some(c),
            d: None,
            e: None,
        }
    }

    pub fn c(c: net::Conn) -> Self {
        MapResult {
            a: None,
            b: None,
            c: Some(c),
            d: None,
            e: None,
        }
    }
    pub fn from_err(e: io::Error) -> Self {
        MapResult {
            a: None,
            b: None,
            c: None,
            d: None,
            e: Some(e),
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
            c: None,
            d: None,
            e: Some(e),
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
pub trait Mapper: crate::Name {
    /// InAdder 与 OutAdder 由 behavior 区分。
    ///
    /// # InAdder
    ///
    /// 可选地返回 解析出的“目标地址”。一般只在InAdder最后一级产生。
    ///
    /// 返回值的 AddResult.d: OptData 用于 获取关于该层连接的额外信息, 一般情况为None即可
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
    async fn maps(
        &self,
        cid: u32, //state 的 id
        behavior: ProxyBehavior,
        params: MapParams,
    ) -> MapResult;
}

//令 Mapper 实现 Sync，否则报错
pub trait MapperSync: Mapper + Send + Sync + Debug {}
impl<T: Mapper + Send + Sync + Debug> MapperSync for T {}

pub type MapperBox = Box<dyn MapperSync>; //必须用Box,不能是 Arc

/// 一种 Mapper 的容器
pub trait MappersVec {
    fn get_mappers_vec(&self) -> &Vec<MapperBox>;

    fn push_mapper(&mut self, adder: MapperBox);
}

/// TcpInAccumulator 是 inadders的累加器，在每一次 处理 新的 in 连接时都会被调用。
///
/// cid 为 跟踪 该连接的 标识
/// 返回的元组包含新的 Conn 和 可能的目标地址
///
/// 用途：从listen得到的tcp开始，一层一层往上加，直到加到能解析出代理目标地址为止
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
pub struct TcpInAccumulator<'a> {
    phantom: std::marker::PhantomData<&'a i32>,
}

pub struct AccumulateResult {
    pub a: Option<net::Addr>,
    pub b: Option<BytesMut>,
    pub c: Option<net::Conn>,
    pub d: Vec<OptData>,
    pub e: Option<io::Error>,
}

impl<'a> TcpInAccumulator<'a> {
    ///  accumulate 是一个作用很强的函数
    /// extra_data_vec 若不为空，其须与 inadders提供同数量的元素, 否则
    /// 将panic
    pub async fn accumulate<IterInMapperBoxRef, IterOptData>(
        cid: u32,
        initial_conn: net::Conn,
        mut mappers: IterInMapperBoxRef,
        mut hyperparameter_vec: Option<IterOptData>,
    ) -> AccumulateResult
    where
        IterInMapperBoxRef: Iterator<Item = &'a MapperBox>,
        IterOptData: Iterator<Item = OptData>,
    {
        let mut last_r: MapResult = MapResult {
            a: None,
            b: None,
            c: Some(initial_conn),
            d: None,
            e: None,
        };

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
                        hyperparameter: if let Some(v) = hyperparameter_vec.as_mut() {
                            v.next().unwrap()
                        } else {
                            None
                        },
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
                            cid,
                            ProxyBehavior::DECODE,
                            MapParams {
                                c: Stream::TCP(last_r.c.unwrap()),
                                a: last_r.a,
                                b: last_r.b,
                                d: input_data,
                            },
                        )
                        .await;

                    calculated_output_vec.push(last_r.d);

                    if last_r.c.is_none() || last_r.e.is_some() {
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
        };
    }
}

/// 类似 TcpInAccumulator , 这是一个作用很强的累加器，
///
/// 一般 【中同层是用不到 target_addr的，只有最后一层会用到，即，
///只有代理层会用到目标地址】
///

pub struct TcpOutAccumulator<'a> {
    phantom: std::marker::PhantomData<&'a i32>,
}

impl<'a> TcpOutAccumulator<'a> {
    pub async fn accumulate<IterOutMapperBoxRef>(
        cid: u32,
        base: net::Conn,
        mut outmappers: IterOutMapperBoxRef,
        target_addr: Option<net::Addr>,
        early_data: Option<BytesMut>,
    ) -> io::Result<(net::Conn, Option<net::Addr>, Vec<OptData>)>
    where
        IterOutMapperBoxRef: Iterator<Item = &'a MapperBox>,
    {
        let mut extra_data_vec = Vec::new();

        let mut last_r = MapResult {
            a: target_addr,
            b: early_data,
            c: Some(base),
            d: None,
            e: None,
        };

        // 与 InAccumulator 相反, early_data 在传入第一层后就会消失
        let first_adder = outmappers.next();

        match first_adder {
            Some(fa) => {
                last_r = fa
                    .maps(
                        cid,
                        ProxyBehavior::ENCODE,
                        MapParams {
                            c: Stream::TCP(last_r.c.unwrap()),
                            a: last_r.a,
                            b: last_r.b,
                            d: None,
                        },
                    )
                    .await;

                extra_data_vec.push(None);

                loop {
                    let oadder = outmappers.next();
                    match oadder {
                        Some(adder) => {
                            last_r = adder
                                .maps(
                                    cid,
                                    ProxyBehavior::ENCODE,
                                    MapParams {
                                        c: Stream::TCP(last_r.c.unwrap()),
                                        a: last_r.a,
                                        b: None,
                                        d: None,
                                    },
                                )
                                .await;
                            extra_data_vec.push(None);
                        }
                        None => {
                            return Ok((last_r.c.unwrap(), last_r.a, extra_data_vec));
                        }
                    }
                }
            }
            None => {
                if let Some(ed) = last_r.b {
                    use tokio::io::AsyncWriteExt;
                    last_r.c.as_mut().unwrap().write(&ed).await?;
                }
                return Ok((last_r.c.unwrap(), last_r.a, extra_data_vec));
            }
        }
    }
}
