/*!
 *
ruci is a proxy abstraction framework, that abstracts the progress of network proxy
by mod user, net , map and relay.

uses tokio.

See doc of mod map for the basic proxy progress abstraction.

Refer to rucimp sub crate for config file format related implements and for more proxy protocol implements.

*/

pub mod map;
pub mod net;
pub mod relay;
pub mod user;
pub mod utils;

pub const VERSION: &str = "0.0.3";

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
