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
