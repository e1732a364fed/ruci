/*!
provide facilities for folding dynamic chain

fold 模块是整个 ruci 链式架构的最核心部分

the mod won't store dynamic data during folding.

几个关键部分: [`MIter`],  [`DynIterator`],  [`DMIterBox`], [`FoldParams`], [`FoldResult`], [`fn@fold`], [`fold_from_start`],

*/

use tracing::{debug, info, warn, Level};

use super::*;

/// static Iterator for [`MapperBox`]
pub trait MIter: Iterator<Item = Arc<MapperBox>> + DynClone + Send + Sync + Debug {}
impl<T: Iterator<Item = Arc<MapperBox>> + DynClone + Send + Sync + Debug> MIter for T {}
dyn_clone::clone_trait_object!(MIter);

pub type MIterBox = Box<dyn MIter>;

/// dynamic Iterator for [`MapperBox`], can get different next item if the
/// input data is different
///
/// DynIterator is uncountable, because it's input is dynamic, it's
/// output is also dynamic
///
/// if you want to count it, you might use get_miter to try to get MIterBox first
///
pub trait DynIterator {
    fn next_with_data(&mut self, cid: CID, data: OVOD) -> Option<Arc<MapperBox>>;

    fn next(&mut self) -> Option<Arc<MapperBox>> {
        self.next_with_data(CID::default(), None)
    }

    fn get_miter(&self) -> Option<MIterBox> {
        None
    }

    fn requires_no_data(&self) -> bool {
        false
    }
}

/// async version of [`DynIterator`]
pub trait DMIter: DynIterator + DynClone + Send + Sync + Debug {}
impl<T: DynIterator + DynClone + Send + Sync + Debug> DMIter for T {}
dyn_clone::clone_trait_object!(DMIter);

pub type DMIterBox = Box<dyn DMIter>;

pub type OVOD = Option<Vec<Option<Box<dyn Data>>>>;

/// 包装 [`MIterBox`] 以使其支持 [`DynIterator`]
#[derive(Debug, Clone)]
pub struct DynMIterWrapper(pub MIterBox);

impl DynIterator for DynMIterWrapper {
    fn next_with_data(&mut self, _cid: CID, _data: OVOD) -> Option<Arc<MapperBox>> {
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

/// 包装 [`std::vec::IntoIter<Arc<MapperBox>>`] 以使其支持 [`DynIterator`]
///
/// 比 [`DynMIterWrapper`] 少一层装箱
#[derive(Debug, Clone)]
pub struct DynVecIterWrapper(pub std::vec::IntoIter<Arc<MapperBox>>);

impl DynIterator for DynVecIterWrapper {
    fn next_with_data(&mut self, _cid: CID, _data: OVOD) -> Option<Arc<MapperBox>> {
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

/// FoldResult won't store dynamic data
pub struct FoldResult {
    pub a: Option<net::Addr>,
    pub b: Option<BytesMut>,
    pub c: Stream,
    pub d: Vec<Option<Box<dyn Data>>>,
    pub e: Option<anyhow::Error>,

    /// 代表 迭代完成后, 最终的 cid
    pub id: CID,

    pub chain_tag: String,

    // 累加后剩余的iter(用于一次加法后产生了 Generator 的情况)
    pub left_mappers_iter: DMIterBox,

    pub no_timeout: bool,

    #[cfg(feature = "trace")]
    pub trace: Vec<String>, // table of Names of each Mapper during accumulation.

    pub shutdown_rx: Option<oneshot::Receiver<()>>,
}

impl Debug for FoldResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FoldResult")
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

/// cid 为 跟踪 该连接的 标识
pub struct FoldParams {
    pub cid: CID,
    pub behavior: ProxyBehavior,
    pub initial_state: MapResult,
    pub mappers: DMIterBox,

    pub chain_tag: String,

    #[cfg(feature = "trace")]
    pub trace: Vec<String>,
}

///  fold 是一个作用很强的函数,是 mappers 的累加器
///
/// 它的做法类似 Iterator 的 fold
///
/// 返回的 FoldResult 包含新的 流 和 可能的目标地址
///
/// behavior为 DECODE 的 一般行为: 从listen得到的ip/tcp/udp/uds开始, 一层一层往上加, 直到加到能解析出代理目标地址为止
///
/// 一般 【中同层是返回的 target_addr都是None, 只有最后一层会返回出目标地址, 即,
///只有代理层会有目标地址】
///
///
/// fold 只适用于 不含 Stream::Generator 的情况, 即 累加不会
/// 造成分支.
///
/// 结果中 Stream为 None 或 一个 Stream::Generator , 或e不为None时, 将退出累加
///
pub async fn fold(params: FoldParams) -> FoldResult {
    let cid = params.cid;
    let initial_state = params.initial_state;
    let mut mappers = params.mappers;

    #[cfg(feature = "trace")]
    let mut trace = params.trace;

    let mut last_r: MapResult = initial_state;

    let mut calculated_output_vec: Vec<Option<Box<dyn Data>>> = Vec::new();
    calculated_output_vec.push(last_r.d);

    let mut tag: String = params.chain_tag;

    loop {
        let adder = if mappers.requires_no_data() {
            mappers.next()
        } else {
            mappers.next_with_data(cid.clone(), Some(calculated_output_vec.clone()))
        };

        let adder = match adder {
            Some(a) => a,
            None => break,
        };

        if tracing::enabled!(Level::DEBUG) {
            debug!(cid = %cid, mapper = adder.name(), behavior = ?params.behavior, "folding",)
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

    FoldResult {
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

        no_timeout: last_r.no_timeout,

        #[cfg(feature = "trace")]
        trace,

        shutdown_rx: last_r.shutdown_rx,
    }
}

/// blocking.
///
/// 先调用第一个 mapper 生成 流发生器, 然后调用 [`in_iter_fold_forever`]
///
/// 但如果 第一个 mapper 生成的不是流发生器而是普通的流, 则会调用 普通的
/// fold, 累加结束后就会返回
///
///
pub async fn fold_from_start(
    in_cid: CID,
    result_dealer: tokio::sync::mpsc::Sender<FoldResult>,
    shutdown_rx: oneshot::Receiver<()>,

    mut inmappers: DMIterBox,
    o_gtr: Option<Arc<GlobalTrafficRecorder>>,
) -> anyhow::Result<()> {
    let first = inmappers
        .next_with_data(in_cid.clone(), None)
        .expect("has first inmapper");
    let first_r = first
        .maps(
            in_cid.clone(),
            ProxyBehavior::DECODE,
            MapParams::builder().shutdown_rx(shutdown_rx).build(),
        )
        .await;
    let first_tag = first.get_chain_tag().to_string();
    if let Some(e) = first_r.e {
        let e = e.context(format!("fold_from_start failed, tag: {} ", first_tag));
        //use {:#} to show full chain of anyhow::Error

        warn!(cid = %in_cid,"{:#} ", e);
        return Err(e);
    }

    if let Stream::Generator(stream_generator) = first_r.c {
        in_iter_fold_forever(InIterFoldForeverParams {
            cid: in_cid,
            stream_generator,
            result_dealer,
            dmiter: inmappers,
            o_gtr,
            first_tag,

            #[cfg(feature = "trace")]
            trace: vec![first.name().to_string()],
        })
        .await;
    } else {
        match &first_r.c {
            Stream::None => {
                warn!(
                    cid = %in_cid,
                    "fold_from_start: no input stream, still trying to fold"
                )
            }
            _ => {
                debug!(
                    cid = %in_cid,
                    "fold_from_start: not a stream generator, will fold directly.",
                );
            }
        }
        let cid = in_cid.clone_push(o_gtr);
        tokio::spawn(async move {
            let r = fold(FoldParams {
                cid,
                behavior: ProxyBehavior::DECODE,
                initial_state: first_r,
                mappers: inmappers,
                chain_tag: first_tag,

                #[cfg(feature = "trace")]
                trace: vec![first.name().to_string()],
            })
            .await;
            let _ = result_dealer.send(r).await;
        });
    };
    Ok(())
}

pub struct InIterFoldForeverParams {
    pub cid: CID,
    pub stream_generator: tokio::sync::mpsc::Receiver<MapResult>,
    pub result_dealer: tokio::sync::mpsc::Sender<FoldResult>,
    pub dmiter: DMIterBox,
    pub o_gtr: Option<Arc<GlobalTrafficRecorder>>,
    pub first_tag: String,

    #[cfg(feature = "trace")]
    pub trace: Vec<String>,
}

/// blocking until stream_generator got closed.
///
/// 用于 已知一个初始点为 Stream::Generator (rx), 向其所有子连接进行accumulate,
/// 直到遇到结果中 Stream为 None 或 一个 Stream::Generator, 或e不为None
///
/// 每一条子连接都使用 dmiter 的克隆, 并在 cid 基础上push生成新的 CIDChain
///
/// 将每一条子连接的accumulate 结果 用 tx 发送出去; 会对 cid 用 clone_push
/// 添加新项
///
/// 如果子连接又是一个 Stream::Generator, 则会继续调用 自己 进行递归
///
pub async fn in_iter_fold_forever(params: InIterFoldForeverParams) {
    let mut rx = params.stream_generator;
    let cid = params.cid;
    let tx = params.result_dealer;
    let dmiter = params.dmiter;
    let o_gtr = params.o_gtr;

    loop {
        let opt_stream_info = rx.recv().await;

        let new_stream_info = match opt_stream_info {
            Some(s) => s,
            None => {
                //debug!(cid = %cid, "got None, will break");
                break;
            }
        };

        let new_cid = if let Some(c) = &new_stream_info.new_id {
            c.clone()
        } else {
            cid.clone_push(o_gtr.clone())
        };

        if tracing::enabled!(Level::INFO) {
            if let Some(d) = &new_stream_info.d {
                info!(
                    cid = %cid,
                    new_cid = %new_cid,
                    data = ?d,
                    "new accepted stream"
                );
            } else {
                info!(
                    cid = %cid,
                    new_cid = %new_cid,
                    "new accepted stream"
                );
            }
        }

        spawn_fold_forever(SpawnFoldForeverParams {
            cid: new_cid,
            new_stream_info,
            miter: dmiter.clone(),
            tx: tx.clone(),
            o_gtr: o_gtr.clone(),
            first_tag: params.first_tag.clone(),

            #[cfg(feature = "trace")]
            trace: params.trace.clone(),
        });
    }
}

struct SpawnFoldForeverParams {
    cid: CID,
    new_stream_info: MapResult,
    miter: DMIterBox,
    tx: tokio::sync::mpsc::Sender<FoldResult>,
    o_gtr: Option<Arc<GlobalTrafficRecorder>>,
    pub first_tag: String,

    #[cfg(feature = "trace")]
    pub trace: Vec<String>,
}

// solve async recursive spawn issue by :
//
// https://github.com/tokio-rs/tokio/issues/2394
fn spawn_fold_forever(params: SpawnFoldForeverParams) {
    let cid = params.cid;
    let tx = params.tx;
    let miter = params.miter;
    let o_gtr = params.o_gtr;

    tokio::spawn(async move {
        let r = fold(FoldParams {
            cid,
            behavior: ProxyBehavior::DECODE,
            initial_state: params.new_stream_info,
            mappers: miter,

            chain_tag: params.first_tag,

            #[cfg(feature = "trace")]
            trace: params.trace,
        })
        .await;

        if let Stream::Generator(rx) = r.c {
            let cid = r.id;

            debug!(cid = %cid, "spawn_acc_forever recursive");
            in_iter_fold_forever(InIterFoldForeverParams {
                cid,

                stream_generator: rx,
                result_dealer: tx,
                dmiter: r.left_mappers_iter.clone(),
                o_gtr,
                first_tag: r.chain_tag,

                #[cfg(feature = "trace")]
                trace: r.trace,
            })
            .await;
        } else {
            let _ = tx.send(r).await;
        }
    });
}
