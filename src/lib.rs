/*!
 *
ruci 库是一个配置无关的代理框架, 通过 user, net 和 map 模块 实现了代理过程的抽象,
并在 relay 模块提供了一些转发过程的参考.

配置相关的进一步实现请参阅 rucimp

具体的关键抽象概念请查看 map 模块文档

异步库用 tokio

*/

use std::{any::Any, sync::Arc};

use parking_lot::Mutex;

pub mod map;
pub mod net;
pub mod relay;
pub mod user;

pub const VERSION: &str = "0.0.1";

/// many types in ruci have a name.
/// 约定：使用小写字母+下划线的形式
pub trait Name {
    fn name(&self) -> &str;
}

impl<T: Name + ?Sized> Name for Box<T> {
    fn name(&self) -> &str {
        (**self).name()
    }
}

pub type AnyS = dyn Any + Send + Sync; // 加 Send+ Sync 以支持多线程
pub type AnyBox = Box<AnyS>;
pub type AnyArc = Arc<Mutex<AnyS>>;
