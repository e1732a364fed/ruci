/*!
Defines Dynamic Chain.

动态链的 iter 每次调用时, 会动态地返回一种Mapper
只有运行时才能知晓一条链是由哪些 Mapper 所组成, 所以无法用 Vec等类型表示,
只能用 Iterator 表示

不过, 有时会有这种情况: 动态链由几部分 静态链组成, 其中两个静态链之间的连接
是动态的

这里将这种链叫做 "Partial/Finite Dynamic Chain", 把完全动态的链叫做
"Complete/Infinite Dynamic Chain"

Partial 的状态是有限的 (即有限状态机 FSM),  Complete 的状态是无限的,
(即无限状态机)

部分动态例子1: 一个 tcp 到一个 tls 监听 ，这部分是静态的，之后根据 tls 的 alpn 结果
进行分支，两个子分支后面也是静态的，但这个判断是动态的

 */
use std::{fmt::Debug, sync::Arc};

use dyn_clone::DynClone;
use ruci::{
    map::{
        fold::{DynIterator, OVOD},
        MapperBox,
    },
    net::CID,
};

/// Complete Dynamic (Infinite) Chain that uses a number to indicate the current state
#[derive(Clone, Debug)]
pub struct IndexInfinite {
    pub tag: String,

    pub generator: Box<dyn IndexNextMapperGenerator>,

    /// current_state_index 是当前状态的指示
    ///
    /// 虽说是无限动态, 但实际用数字表示的最大状态数是i64的上限,
    /// 但实际使用肯定够用了
    ///
    pub current_state_index: i64,
}

impl IndexInfinite {
    pub fn new(tag: String, generator: Box<dyn IndexNextMapperGenerator>) -> Self {
        IndexInfinite {
            tag,
            generator,
            current_state_index: -1,
        }
    }
}

pub type IndexMapperBox = (i64, Option<Arc<MapperBox>>); //MapperBox 和它的 索引

/// 若返回的 index 小于0, 则指示迭代结束
///
pub trait IndexNextMapperGenerator: DynClone + Debug + Send + Sync {
    fn next_mapper(
        &mut self,
        cid: CID,
        this_state_index: i64,
        data: OVOD,
    ) -> Option<IndexMapperBox>;
}

dyn_clone::clone_trait_object!(IndexNextMapperGenerator);

impl DynIterator for IndexInfinite {
    fn next_with_data(&mut self, cid: CID, data: OVOD) -> Option<Arc<MapperBox>> {
        let oi = self
            .generator
            .next_mapper(cid, self.current_state_index, data);
        match oi {
            Some(ib) => {
                let i = ib.0;
                if i < 0 {
                    return None;
                }
                self.current_state_index = i;

                ib.1
            }
            None => None,
        }
    }
}

/// 有界部分动态链, 即米利型有限状态机, Mealy machine
#[derive(Debug, Clone)]
pub struct Finite {
    /// finite set
    ///
    /// 每一个 MapperBox 都是静态的, 在 Vec中有固定的序号.
    /// 根据 selector 返回的序号 决定下一个调用哪一个.
    /// selector 返回 None 表示  链终止
    ///
    pub mb_vec: Vec<Arc<MapperBox>>,

    /// transition function
    pub selector: Box<dyn NextSelector>,

    /// current state
    pub current_index: i64,
    //pub history: Vec<usize>,
}

/// 即FSM的 状态转移函数
pub trait NextSelector: Debug + DynClone + Send + Sync {
    ///
    /// acts like a state-transition function, data and this_state_index is the current state
    ///
    /// initial state is None and -1.
    ///
    /// 初始index 传入 -1. 如果 返回值为 None, 或 返回值<0 或 返回值 大于最大索引值,
    /// 则意味着链终止
    fn next_index(&self, this_state_index: i64, data: OVOD) -> Option<i64>;
}
dyn_clone::clone_trait_object!(NextSelector);

impl DynIterator for Finite {
    fn next_with_data(&mut self, _cid: CID, data: OVOD) -> Option<Arc<MapperBox>> {
        let oi = self.selector.next_index(self.current_index, data);
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
                //self.history.push(iu);
                self.mb_vec.get(iu).cloned()
            }
            None => None,
        }
    }
}
