/*!
 * Defines an general steganography protocol.
 *
 *
 * 隐写协议的实现，在 [`SteganographyProcessor`] 中。
 *
 * 通用的隐写协议实现原理：
 *
 * 隐写协议对于每一个数据包，都生成一个写序列和一个读序列。
 *
 * 也就是说，对于一个 要写入的 w,生成的是一个 (w1, r1, w2, r2, ...) 的 读写序列
 * 对于一个要读取的 r,生成的是一个 (r1, w1, r2, w2, ...) 的 读写序列
 *
 * 而 SteganographyProcessor 中的实现 要保证所生成的序列 满足所要隐写协议的统计特征。
 *
 */

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use bytes::BytesMut;
use dyn_clone::DynClone;
use ruci::{
    map::*,
    net::{self, CID},
    Name,
};

pub mod conn;

use conn::GeneralConn;
use tokio::io::AsyncReadExt;
use tracing::debug;

/// 写序列状态
#[derive(Debug)]
pub struct WriteStep {
    pub is_write: bool, // true: 执行 w* 操作, false: 执行 r* 操作
    pub index: usize,   // 对应 write_packets 或 read_lengths 的索引
}

#[derive(Debug)]
pub struct WriteSequence {
    pub write_packets: Vec<Vec<u8>>,
    pub read_lengths: Vec<usize>,
    pub(crate) current_step: WriteStep,
}

/// 读序列状态
#[derive(Debug)]
pub struct ReadStep {
    pub is_read: bool, // true: 执行 r* 操作, false: 执行 w* 操作
    pub index: usize,  // 对应 read_packets 或 write_packets 的索引
}

#[derive(Debug)]
pub struct ReadSequence {
    pub write_packets: Vec<Vec<u8>>,
    pub read_lengths: Vec<usize>,
    pub read_packets: Vec<Vec<u8>>,
    pub current_step: ReadStep,
}

impl ReadSequence {
    fn advance_to_next_write(&mut self) {
        self.current_step.is_read = false;
    }

    fn advance_to_next_read(&mut self) {
        self.current_step.is_read = true;
        self.current_step.index += 1;
    }
}

impl WriteSequence {
    fn advance_to_next_read(&mut self) {
        self.current_step.is_write = false;
    }

    fn advance_to_next_write(&mut self) {
        self.current_step.is_write = true;
        self.current_step.index += 1;
    }
}

#[derive(Debug)]
pub enum ParsedResult {
    Write(WriteSequence),
    Read {
        sequence: ReadSequence,
        addr: Option<net::Addr>,
    },
}

/// 隐写协议处理器
///
/// 这里是实际的隐写协议的实现之处
#[async_trait]
pub trait SteganographyProcessor: Send + Sync + Name + DynClone {
    /// 在客户端，处理目标地址和数据，生成写序列
    /// 在服务端，处理读到的客户端握手的写序列中的第一个包，生成读序列
    async fn generate_sequence(
        &self,
        data: &[u8],
        target_addr: Option<net::Addr>,
        is_handshake: bool,
        is_read: bool,
    ) -> Result<ParsedResult>;

    /// 解密读取到的数据
    async fn decrypt_read_sequence(&self, data: Vec<u8>) -> Result<Vec<u8>>;
}
dyn_clone::clone_trait_object!(SteganographyProcessor);

#[derive(Clone)]
pub struct GeneralMap {
    pub is_server: bool,

    pub processor: Arc<Box<dyn SteganographyProcessor>>,
    cached_name: String,
}

impl std::fmt::Debug for GeneralMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GeneralSteganographyMap")
    }
}

impl GeneralMap {
    pub fn from_processor(
        is_server: bool,
        processor: Arc<Box<dyn SteganographyProcessor>>,
    ) -> Self {
        let mut map = Self {
            is_server,
            processor,
            cached_name: "".to_string(),
        };
        map.generate_name();
        map
    }

    /// 在客户端，处理目标地址和数据，生成写序列
    /// 在服务端，处理读到的客户端握手的写序列中的第一个包，生成读序列
    async fn generate_sequence(
        &self,
        data: &[u8],
        target_addr: Option<net::Addr>,
        is_handshake: bool,
        is_read: bool,
    ) -> Result<ParsedResult> {
        self.processor
            .generate_sequence(data, target_addr, is_handshake, is_read)
            .await
    }

    /// 解密从隐写协议中读取的数据
    async fn decrypt_read_sequence(&self, combined_data: Vec<u8>) -> Result<Vec<u8>> {
        self.processor.decrypt_read_sequence(combined_data).await
    }

    /// generated cached name from its processor
    fn generate_name(&mut self) {
        self.cached_name = format!(
            "general_steganography_map[processor: {}]",
            self.processor.name()
        );
    }
}

impl Name for GeneralMap {
    fn name(&self) -> &str {
        self.cached_name.as_str()
    }
}

#[async_trait]
impl Map for GeneralMap {
    async fn maps(&self, _cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match behavior {
            ProxyBehavior::ENCODE => {
                let conn = GeneralConn::new(
                    params.c.try_unwrap_tcp().unwrap(),
                    self.clone(),
                    params.b,
                    params.a,
                );
                MapResult::new_c(Box::new(conn)).build()
            }
            ProxyBehavior::DECODE => {
                let mut conn = GeneralConn::new(
                    params.c.try_unwrap_tcp().unwrap(),
                    self.clone(),
                    params.b,
                    params.a,
                );

                let mut buf = BytesMut::zeroed(2048); //todo: change this
                let r = conn.read_buf(&mut buf).await;
                match r {
                    Ok(n) => {
                        debug!("general steganography: server read handshake success, {n}");
                        let ta = conn.target_addr.take();

                        MapResult::new_c(Box::new(conn)).b(Some(buf)).a(ta).build()
                    }
                    Err(e) => MapResult::from_e(anyhow::anyhow!(
                        "general steganography: server read handshake failed, {e}"
                    )),
                }
            }
            ProxyBehavior::UNSPECIFIED => MapResult::from_e(anyhow::anyhow!(
                "general steganography: Unspecified behavior is not supported"
            )),
        }
    }
}
