/*!
Defines some [`ruci::map::Map`] s
 */

#[cfg(all(feature = "sockopt", target_os = "linux"))]
pub mod opt_net;

#[cfg(all(feature = "sockopt", target_os = "linux"))]
pub mod tproxy;

pub mod h2;
#[cfg(any(feature = "use-native-tls", feature = "native-tls-vendored"))]
pub mod native_tls;
pub mod ws;

pub mod quic_common;

#[cfg(feature = "quic")]
pub mod quic;

#[cfg(feature = "rustls21")]
pub mod rustls21;

#[cfg(feature = "quinn")]
pub mod quinn;
