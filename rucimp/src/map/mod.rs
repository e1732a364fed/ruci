/*!
Defines some [`ruci::map::Map`]s.
 */

pub mod h2;
pub mod quic_common;
pub mod recorder;
pub mod ws;

#[cfg(feature = "steganography")]
pub mod spe1;

#[cfg(feature = "quic")]
pub mod quic;

#[cfg(feature = "quinn")]
pub mod quinn;

#[cfg(any(feature = "use-native-tls", feature = "native-tls-vendored"))]
pub mod native_tls;
#[cfg(feature = "rustls21")]
pub mod rustls21;

#[cfg(feature = "sockopt")]
pub mod opt_net;

#[cfg(all(feature = "sockopt", target_os = "linux"))]
pub mod tproxy;

#[cfg(feature = "smoltcp")]
pub mod tcp_ip_stack_smoltcp;

pub mod tcp_ip_stack_lwip;

#[cfg(any(feature = "lua", feature = "lua54"))]
pub mod lua;
