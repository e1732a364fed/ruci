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
 * 注意，作为 纯隐写协议，这里不考虑任何 “代理协议”、“加密”的功能。若要需要这些功能，在链尾添加相应的 Map 即可。
 *
 */

use std::sync::Arc;

use async_trait::async_trait;
use bytes::BytesMut;
use rainbow::NetworkSteganographyProcessor;
use ruci::{map::*, net::CID};

pub mod conn;

use conn::GeneralConn;
use tokio::io::AsyncReadExt;
use tracing::debug;

#[derive(Clone)]
pub struct GeneralMap {
    pub is_server: bool,

    pub processor: Arc<Box<dyn NetworkSteganographyProcessor>>,
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
        processor: Arc<Box<dyn NetworkSteganographyProcessor>>,
        name: &str,
    ) -> Self {
        let mut map = Self {
            is_server,
            processor,
            cached_name: "".to_string(),
        };
        map.generate_name(name);
        map
    }

    /// generated cached name from its processor
    fn generate_name(&mut self, name: &str) {
        self.cached_name = format!("general_steganography_map[processor: {name}]",);
    }
}

impl Display for GeneralMap {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.cached_name)
    }
}

#[async_trait]
impl Map for GeneralMap {
    async fn maps(&self, _cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match behavior {
            ProxyBehavior::ENCODE => {
                let conn =
                    GeneralConn::new(params.c.try_unwrap_tcp().unwrap(), self.clone(), params.b);
                MapResult::new_c(Box::new(conn)).a(params.a).build()
            }
            ProxyBehavior::DECODE => {
                let mut conn =
                    GeneralConn::new(params.c.try_unwrap_tcp().unwrap(), self.clone(), params.b);

                let mut buf = BytesMut::zeroed(2048); //todo: change this
                let r = conn.read_buf(&mut buf).await;
                match r {
                    Ok(n) => {
                        debug!("general steganography: server read handshake success, {n}");

                        MapResult::new_c(Box::new(conn))
                            .b(Some(buf))
                            .a(params.a)
                            .build()
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
