/*!
* 提供一些累加方法

struct AccumulateResult

* function accumulate , accumulate_from_start
*/
use super::*;

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
    // 累加后剩余的iter(用于一次加法后产生了 Generator 的情况)
    //pub left_mappers_iter: MIterBox,
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
        //left_mappers_iter: mappers,
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
