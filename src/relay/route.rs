use crate::map::{AnyData, MapperBox};

/// Send + Sync to use in async
pub trait OutSelector<'a, T>: Send + Sync
where
    T: Iterator<Item = &'a MapperBox>,
{
    fn select(&self, params: Vec<Option<AnyData>>) -> T;
}

pub struct FixedOutSelector<'a, T>
where
    T: Iterator<Item = &'a MapperBox> + Clone + Send,
{
    pub mappers: T,
}

impl<'a, T> OutSelector<'a, T> for FixedOutSelector<'a, T>
where
    T: Iterator<Item = &'a MapperBox> + Clone + Send + Sync,
{
    fn select(&self, _params: Vec<Option<AnyData>>) -> T {
        self.mappers.clone()
    }
}
