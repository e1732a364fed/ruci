/*!
 *
ruci is a proxy abstraction framework, that abstracts the progress of network proxy
by mod user, net , map and relay.

uses tokio.

See doc of mod map for the basic proxy progress abstraction.

Refer to rucimp sub crate for config file format related implements and for more proxy protocol implements.

*/

use std::{any::Any, sync::Arc};

use bytes::BytesMut;
use parking_lot::Mutex;

pub mod map;
pub mod net;
pub mod relay;
pub mod user;

pub const VERSION: &str = "0.0.2";

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

pub type AnyS = dyn Any + Send + Sync; //  Send + Sync for multi-thread
pub type AnyBox = Box<AnyS>;
pub type AnyArc = Arc<Mutex<AnyS>>;

pub fn buf_to_ob(b: BytesMut) -> Option<BytesMut> {
    if b.is_empty() {
        None
    } else {
        Some(b)
    }
}
