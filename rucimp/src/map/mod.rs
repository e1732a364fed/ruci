/*!
Defines some [`ruci::map::Map`]s.
 */

pub mod h2;
pub mod quic_common;
pub mod ws;

#[cfg(feature = "steganography")]
pub mod steganography;

// #[cfg(feature = "quic")]
// pub mod quic;

#[cfg(feature = "quinn")]
pub mod quinn;

#[cfg(any(feature = "use-native-tls", feature = "native-tls-vendored"))]
pub mod native_tls;

#[cfg(feature = "sockopt")]
pub mod opt_net;

#[cfg(all(feature = "sockopt", target_os = "linux"))]
pub mod tproxy;

#[cfg(any(feature = "lua", feature = "lua54"))]
pub mod lua;

#[cfg(any(feature = "lwip", feature = "smoltcp"))]
pub mod tcp_ip_stack;
