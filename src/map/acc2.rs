/*!
 * provide facilities for accumulating dynamic chain
 *
 * acc2 模块的内容结构与 acc 模块的内容结构是相似的
 */

use super::*;

/// dynamic Iterator, can get different next item if the
/// input data is different
pub trait DynIterator {
    type Item;

    fn next(&mut self, data: Vec<OptVecData>) -> Option<Self::Item>;
}

pub trait DMIter: DynIterator<Item = Arc<MapperBox>> + DynClone + Send + Sync + Debug {}
impl<T: DynIterator<Item = Arc<MapperBox>> + DynClone + Send + Sync + Debug> DMIter for T {}
dyn_clone::clone_trait_object!(DMIter);

pub type DMIterBox = Box<dyn DMIter>;
