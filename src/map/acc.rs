/*!
* provide facilities for accumulating dynamic chain

trait: DynIterator, MIter, DMIter

struct DynMIterWrapper, DynVecIterWrapper, AccumulateResult, AccumulateParams

* function accumulate , accumulate_from_start
*/
use log::debug;

use super::*;

/// static iterator for MapperBox
pub trait MIter: Iterator<Item = Arc<MapperBox>> + DynClone + Send + Sync + Debug {}
impl<T: Iterator<Item = Arc<MapperBox>> + DynClone + Send + Sync + Debug> MIter for T {}
dyn_clone::clone_trait_object!(MIter);

pub type MIterBox = Box<dyn MIter>;

/// dynamic Iterator, can get different next item if the
/// input data is different
///
/// DynIterator is uncountable, because it's input is dynamic, it's
/// output is also dynamic
///
/// if you want to count it, you might use get_miter to try to get MIterBox first
///
pub trait DynIterator {
    fn next_with_data(&mut self, data: Option<Vec<OptVecData>>) -> Option<Arc<MapperBox>>;

    fn next(&mut self) -> Option<Arc<MapperBox>> {
        self.next_with_data(None)
    }

    fn get_miter(&self) -> Option<MIterBox> {
        None
    }

    fn requires_no_data(&self) -> bool {
        false
    }
}

pub trait DMIter: DynIterator + DynClone + Send + Sync + Debug {}
impl<T: DynIterator + DynClone + Send + Sync + Debug> DMIter for T {}
dyn_clone::clone_trait_object!(DMIter);

pub type DMIterBox = Box<dyn DMIter>;

/// 包装 MIterBox 以使其支持 DynIterator
#[derive(Debug, Clone)]
pub struct DynMIterWrapper(pub MIterBox);

impl DynIterator for DynMIterWrapper {
    fn next_with_data(&mut self, _data: Option<Vec<OptVecData>>) -> Option<Arc<MapperBox>> {
        self.0.next()
    }

    fn next(&mut self) -> Option<Arc<MapperBox>> {
        self.0.next()
    }

    fn get_miter(&self) -> Option<MIterBox> {
        Some(self.0.clone())
    }

    fn requires_no_data(&self) -> bool {
        true
    }
}

/// 包装 std::vec::IntoIter<Arc<MapperBox>> 以使其支持 DynIterator
///
/// 比 DynMIterWrapper 少一层装箱
#[derive(Debug, Clone)]
pub struct DynVecIterWrapper(pub std::vec::IntoIter<Arc<MapperBox>>);

impl DynIterator for DynVecIterWrapper {
    fn next_with_data(&mut self, _data: Option<Vec<OptVecData>>) -> Option<Arc<MapperBox>> {
        self.0.next()
    }

    fn next(&mut self) -> Option<Arc<MapperBox>> {
        self.0.next()
    }

    fn get_miter(&self) -> Option<MIterBox> {
        Some(Box::new(self.0.clone()))
    }

    fn requires_no_data(&self) -> bool {
        true
    }
}

pub struct AccumulateResult {
    pub a: Option<net::Addr>,
    pub b: Option<BytesMut>,
    pub c: Stream,
    pub d: Vec<OptVecData>,
    pub e: Option<anyhow::Error>,

    /// 代表 迭代完成后，最终的 cid
    pub id: CID,

    pub chain_tag: String,

    // 累加后剩余的iter(用于一次加法后产生了 Generator 的情况)
    pub left_mappers_iter: DMIterBox,

    #[cfg(feature = "trace")]
    pub trace: Vec<String>, // table of Names of each Mapper during accumulation.
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
            .finish()
    }
}

pub struct AccumulateParams {
    pub cid: CID,
    pub behavior: ProxyBehavior,
    pub initial_state: MapResult,
    pub mappers: DMIterBox,

    #[cfg(feature = "trace")]
    pub trace: Vec<String>,
}

///  accumulate 是一个作用很强的函数,是 mappers 的累加器
///
/// 它的做法类似 Iterator 的 fold
///
/// cid 为 跟踪 该连接的 标识
/// 返回的元组包含新的 Conn 和 可能的目标地址
///
/// decode: 用途： 从listen得到的ip/tcp/udp/uds开始, 一层一层往上加, 直到加到能解析出代理目标地址为止
///
/// 一般 【中同层是返回的 target_addr都是None, 只有最后一层会返回出目标地址, 即,
///只有代理层会有目标地址】
///
///
/// accumulate 只适用于 不含 Stream::Generator 的情况,
///
/// 结果中 Stream为 None 或 一个 Stream::Generator , 或e不为None时, 将退出累加
///
/// 能生成 Stream::Generator 说明其 behavior 为 DECODE
///
pub async fn accumulate(params: AccumulateParams) -> AccumulateResult {
    let cid = params.cid;
    let initial_state = params.initial_state;
    let mut mappers = params.mappers;

    #[cfg(feature = "trace")]
    let mut trace = params.trace;

    let mut last_r: MapResult = initial_state;

    let mut calculated_output_vec: Vec<OptVecData> = Vec::new();

    let mut tag: String = String::new();

    loop {
        let adder = if mappers.requires_no_data() {
            mappers.next()
        } else {
            mappers.next_with_data(Some(calculated_output_vec.clone()))
        };

        let adder = match adder {
            Some(a) => a,
            None => break,
        };

        if log_enabled!(log::Level::Debug) {
            debug!("acc: {cid} , mapper: {}", adder.name())
        }
        last_r = adder
            .maps(
                match last_r.new_id {
                    Some(id) => id,
                    None => cid.clone(),
                },
                params.behavior,
                MapParams {
                    c: last_r.c,
                    a: last_r.a,
                    b: last_r.b,
                    d: calculated_output_vec.clone(),
                    ..Default::default()
                },
            )
            .await;

        if tag.is_empty() {
            let ct = adder.get_chain_tag();

            if !ct.is_empty() {
                tag = ct.to_string();
            }
        }

        calculated_output_vec.push(last_r.d);

        #[cfg(feature = "trace")]
        trace.push(adder.name().to_string());

        if last_r.c.is_none_or_generator() {
            break;
        }
        if last_r.e.is_some() {
            break;
        }
    } //for

    AccumulateResult {
        a: last_r.a,
        b: last_r.b,
        c: last_r.c,
        d: calculated_output_vec,
        e: last_r.e,

        id: match last_r.new_id {
            Some(nid) => nid,
            None => cid,
        },
        left_mappers_iter: mappers,

        chain_tag: tag,

        #[cfg(feature = "trace")]
        trace,
    }
}

/// blocking.
/// 先调用第一个 mapper 生成 流, 然后调用 in_iter_accumulate_forever
///
/// 但如果 第一个 mapper 生成的不是流, 则会调用 普通的 accumulate, 累加结束后就会返回,
/// 不会永远阻塞.
///
pub async fn accumulate_from_start(
    tx: tokio::sync::mpsc::Sender<AccumulateResult>,
    shutdown_rx: oneshot::Receiver<()>,

    mut inmappers: DMIterBox,
    oti: Option<Arc<GlobalTrafficRecorder>>,
) -> anyhow::Result<()> {
    let first = inmappers.next_with_data(None).expect("first inmapper");
    let first_r = first
        .maps(
            CID::default(),
            ProxyBehavior::DECODE,
            MapParams::builder().shutdown_rx(shutdown_rx).build(),
        )
        .await;
    if let Some(e) = first_r.e {
        let e = e.context(format!(
            "accumulate_from_start failed, tag: {} ",
            first.get_chain_tag()
        ));
        //use {:#} to show full chain of anyhow::Error

        warn!("{:#} ", e);
        return Err(e);
    }

    if let Stream::Generator(rx) = first_r.c {
        in_iter_accumulate_forever(InIterAccumulateForeverParams {
            cid: CID::default(),
            rx,
            tx,
            miter: inmappers,
            oti,

            #[cfg(feature = "trace")]
            trace: vec![first.name().to_string()],
        })
        .await;
    } else {
        match &first_r.c {
            Stream::None => {
                warn!("accumulate_from_start: no input stream, still trying to accumulate")
            }
            _ => {
                debug!("accumulate_from_start: not a stream generator, will accumulate directly.",);
            }
        }

        tokio::spawn(async move {
            let r = accumulate(AccumulateParams {
                cid: CID::new_by_opti(oti),
                behavior: ProxyBehavior::DECODE,
                initial_state: first_r,
                mappers: inmappers,

                #[cfg(feature = "trace")]
                trace: vec![first.name().to_string()],
            })
            .await;
            let _ = tx.send(r).await;
        });
    };
    Ok(())
}

struct InIterAccumulateForeverParams {
    cid: CID,
    rx: tokio::sync::mpsc::Receiver<MapResult>,
    tx: tokio::sync::mpsc::Sender<AccumulateResult>,
    miter: DMIterBox,
    oti: Option<Arc<GlobalTrafficRecorder>>,

    #[cfg(feature = "trace")]
    pub trace: Vec<String>,
}

/// blocking until rx got closed.
///
/// 用于 已知一个初始点为 Stream::Generator (rx), 向其所有子连接进行accumulate,
/// 直到遇到结果中 Stream为 None 或 一个 Stream::Generator, 或e不为None
///
///
/// 将每一条子连接的accumulate 结果 用 tx 发送出去;
///
/// 如果子连接又是一个 Stream::Generator, 则会继续调用 自己 进行递归
///
async fn in_iter_accumulate_forever(params: InIterAccumulateForeverParams) {
    let mut rx = params.rx;
    let cid = params.cid;
    let tx = params.tx;
    let miter = params.miter;
    let oti = params.oti;

    loop {
        let opt_stream_info = rx.recv().await;

        let new_stream_info = match opt_stream_info {
            Some(s) => s,
            None => break,
        };

        if log_enabled!(log::Level::Info) {
            info!("{cid}, new accepted stream");
        }

        spawn_acc_forever(SpawnAccForeverParams {
            cid: cid.clone_push(oti.clone()),
            new_stream_info,
            miter: miter.clone(),
            tx: tx.clone(),
            oti: oti.clone(),

            #[cfg(feature = "trace")]
            trace: params.trace.clone(),
        });
    }
}

struct SpawnAccForeverParams {
    cid: CID,
    new_stream_info: MapResult,
    miter: DMIterBox,
    tx: tokio::sync::mpsc::Sender<AccumulateResult>,
    oti: Option<Arc<GlobalTrafficRecorder>>,

    #[cfg(feature = "trace")]
    pub trace: Vec<String>,
}

// solve async recursive spawn issue by :
//
// https://github.com/tokio-rs/tokio/issues/2394
fn spawn_acc_forever(params: SpawnAccForeverParams) {
    let cid = params.cid;
    let tx = params.tx;
    let miter = params.miter;
    let oti = params.oti;

    tokio::spawn(async move {
        let r = accumulate(AccumulateParams {
            cid,
            behavior: ProxyBehavior::DECODE,
            initial_state: params.new_stream_info,
            mappers: miter,

            #[cfg(feature = "trace")]
            trace: params.trace,
        })
        .await;

        if let Stream::Generator(rx) = r.c {
            let cid = r.id;

            debug!("{cid} spawn_acc_forever recursive");
            in_iter_accumulate_forever(InIterAccumulateForeverParams {
                cid,

                rx,
                tx,
                miter: r.left_mappers_iter.clone(),
                oti,

                #[cfg(feature = "trace")]
                trace: r.trace,
            })
            .await;
        } else {
            let _ = tx.send(r).await;
        }
    });
}
