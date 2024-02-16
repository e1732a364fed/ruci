/*!
 * relay 包定义了一种转发逻辑，但是它不是强制性的，可用于参考。
rucimp 中可以有不同的转发逻辑

TcpStream 的版本和 UnixStream 的版本的代码应该是一样的, 但因为要用 shutdown 等 非 trait的方法，所以没用泛型

*/

pub mod cp_tcp;
pub mod tcp;

pub const READ_HANDSHAKE_TIMEOUT: u64 = 15; // 15秒的最长握手等待时间。 //todo: 修改这里
