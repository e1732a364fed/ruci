/*!
 * 这里定义了动态链。动态链的 iter 每次调用时, 会动态地返回一种Mapper
 * 只有运行时才能知晓一条链是由哪些 Mapper 所组成, 所以无法用 Vec等类型表示,
 * 只能用 Iterator 表示
 *
 * 不过, 有时会有这种情况: 动态链由几部分 静态链组成, 其中两个静态链之间的连接
 * 是动态的
 *
 * 我将这种链叫做 "Partial Dynamic Chain", 把完全动态的链叫做 "Complete
 * Dynamic Chain"
 *
 * Partial 是有界的 (Bounded) ,  Complete 是无界的 (Unbounded)
 *
 * 部分动态链比完全动态链更实用
 *
 * 比如，一个 tcp 到一个 tls 监听 ，这部分是静态的，之后根据 tls 的 alpn 结果
 * ，进行分支，两个子分支后面也是静态的，但这个判断是动态的
 *
 * 在完全动态链中，因为完全不知道任何有效信息，将没办法提前初始化，对每个 Inbound
 * 连接都要初始化一遍配置（如加载tls证书等），将是非常低效的
 *
 */
use std::{collections::HashMap, fmt::Debug, sync::Arc};

use async_trait::async_trait;
use dyn_clone::DynClone;
use ruci::map::{acc::DynIterator, MapperBox};

use uuid::Uuid;

/// Complete Dynamic Chain using index
pub struct IndexUnbounded {
    pub selector: Box<dyn IndexNextInMapperGenerator>,

    /// 生成的 新 MapperBox 会存储在 cache 中
    pub cache: Vec<Arc<MapperBox>>,

    pub history: Vec<usize>,

    pub current_index: usize,
}

pub type IndexMapperBox = (usize, Arc<MapperBox>); //MapperBox 和它的 索引

/// 如果产生的是新的, 则其index 为 cache_len
pub trait IndexNextInMapperGenerator {
    fn next_in_mapper(
        &self,
        this_index: usize,
        cache_len: usize,
        data: Option<Vec<ruci::map::OptVecData>>,
    ) -> Option<IndexMapperBox>;
}

// #[async_trait]
// impl DynIterator for IndexUnbounded {
//     async fn next_with_data(
//         &mut self,
//         data: Option<Vec<ruci::map::OptVecData>>,
//     ) -> Option<Arc<MapperBox>> {
//         let cl = self.cache.len();
//         let oi = self.selector.next_in_mapper(self.current_index, cl, data);
//         match oi {
//             Some(ib) => {
//                 let i = ib.0;
//                 self.current_index = i;
//                 self.history.push(i);
//                 if i == cl {
//                     self.cache.push(ib.1.clone());
//                 }
//                 Some(ib.1)
//             }
//             None => None,
//         }
//     }
// }

/// Complete Dynamic Chain using uuid
pub struct UuidUnbounded {
    pub selector: Box<dyn UuidInfiniteNextInMapperGenerator>,

    /// 生成的 新 MapperBox 会存储在 cache 中
    pub cache: HashMap<Uuid, Arc<MapperBox>>,

    pub history: Vec<Uuid>,

    pub current_id: Uuid,
}

pub type UUIDMapperBox = (Uuid, Arc<MapperBox>); //MapperBox 和它的 uuid

pub trait UuidInfiniteNextInMapperGenerator {
    fn next_in_mapper(
        &self,
        this_index: Uuid,
        data: Option<Vec<ruci::map::OptVecData>>,
    ) -> Option<UUIDMapperBox>;
}

/// 有界部分动态链
#[derive(Debug, Clone)]
pub struct Bounded {
    /// 每一个 MapperBox 都是静态的, 在 Vec中有固定的序号.
    /// 第一个会被第一个调用, 之后根据 selector 返回的序
    /// 号 决定下一个调用哪一个. selector 返回 None 表示
    /// 链终止
    ///
    pub mb_vec: Vec<Arc<MapperBox>>,

    pub selector: Box<dyn NextSelector>,

    pub current_index: i64,

    pub history: Vec<usize>,
}

#[async_trait]
pub trait NextSelector: Debug + DynClone + Send + Sync {
    /// 初始index 传入 -1. 如果 返回值为 None, 或 返回值<0 或 返回值 大于最大索引值,
    /// 则意味着链终止
    async fn next_index(
        &self,
        this_index: i64,
        data: Option<Vec<ruci::map::OptVecData>>,
    ) -> Option<i64>;
}
dyn_clone::clone_trait_object!(NextSelector);

#[async_trait]
impl DynIterator for Bounded {
    async fn next_with_data(
        &mut self,
        data: Option<Vec<ruci::map::OptVecData>>,
    ) -> Option<Arc<MapperBox>> {
        let oi = self.selector.next_index(self.current_index, data).await;
        match oi {
            Some(i) => {
                if i < 0 {
                    return None;
                }
                let iu: usize = i as usize;
                if iu >= self.mb_vec.len() {
                    return None;
                }
                self.current_index = i;
                self.history.push(iu);
                self.mb_vec.get(iu).cloned()
            }
            None => None,
        }
    }
}
