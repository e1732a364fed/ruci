/*!
 * relay 包定义了一种转发逻辑，但是它不是强制性的，可用于参考。
具体实现 中可以有不同的转发逻辑

*/

pub mod conn;
pub mod cp_tcp;

pub const READ_HANDSHAKE_TIMEOUT: u64 = 15; // 15秒的最长握手等待时间。 //todo: 修改这里
