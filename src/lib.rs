/*!
ruci is a proxy abstraction framework that abstracts the progress of network proxy
by mod [`user`], [`net`] , [`map`] and [`relay`].

It uses tokio.

See doc of mod [`map`] for the basic proxy progress abstraction.

Refer to rucimp crate for config file format related implements and more proxy protocol implements.

*/

pub mod map;
pub mod net;
pub mod relay;
pub mod user;
pub mod utils;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// 重新导出一些包，以方便其它引用 ruci 的 包 使用

// pub use tokio_rustls;

/// many types in ruci have a name.
/// use lower case letters + underline
pub trait Name {
    fn name(&self) -> &str;
}

impl<T: Name + ?Sized> Name for Box<T> {
    fn name(&self) -> &str {
        (**self).name()
    }
}

impl<T: Name + ?Sized> Name for &mut T {
    fn name(&self) -> &str {
        (**self).name()
    }
}
