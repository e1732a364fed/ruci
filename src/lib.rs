/*!
 *
ruci 库是一个配置无关的代理框架, 通过 user, net 和 map 模块 实现了代理过程的抽象,
并在 relay 模块提供了一些转发过程的参考.

异步库用 tokio

目前用起来和 async_std 的最大的区别是, tokio 的TcpStream 不支持 clone;
async_std的 UdpSocket 少了 poll 方法 (until 24.2.18)
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
