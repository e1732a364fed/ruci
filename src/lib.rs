/*!
 *
异步库用 tokio

目前用起来和 async_std 的最大的区别是，tokio 的TcpStream 不支持 clone
*/

pub mod map;
pub mod net;
pub mod relay;
pub mod user;

pub const VERSION: &str = "0.0.0";

pub trait Name {
    fn name(&self) -> &str;
}
