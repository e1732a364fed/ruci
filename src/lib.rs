/*!
 *
异步库用 tokio

目前用起来和 async_std 的最大的区别是, tokio 的TcpStream 不支持 clone;
async_std的 UdpSocket 少了 poll 方法 (until 24.2.18)
*/

pub mod map;
pub mod net;
pub mod relay;
pub mod user;

pub const VERSION: &str = "0.0.0";

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
