use async_trait::async_trait;
use bytes::BytesMut;
use macro_map::{map_ext_fields, MapExt};
use ruci::map;
use ruci::map::*;
use ruci::net::CID;
use std::io;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::task::Poll;
use std::{fmt::Display, pin::Pin};
use tokio::io::{AsyncRead, AsyncWrite};
// use tracing::{debug, error, info};

use crate::map::recorder::PayloadInfo;

#[map_ext_fields]
#[derive(Debug, Clone, MapExt)]
pub struct InfoEmbedder {
    pub file: Arc<Vec<PayloadInfo>>,
}

impl Display for InfoEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "info_embedder")
    }
}

#[async_trait]
impl Map for InfoEmbedder {
    async fn maps(&self, _cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        if matches!(behavior, ProxyBehavior::UNSPECIFIED) {
            return MapResult::from_err_str(
                "Info Embedder does not support ProxyBehavior::UNSPECIFIED",
            );
        }

        let c = params.c.try_unwrap_tcp().unwrap();

        let info_conn = InfoEmbedConn {
            file: self.file.clone(),
            base: Box::pin(c),
            current_packet_index: Arc::new(AtomicUsize::new(0)),
            behavior,
            first_buf_to_write: params.b,
        };

        MapResult::new_c(Box::new(info_conn)).a(params.a).build()
    }
}

/// 对于 本Conn， Read 是从 已有的文件中获取Info（主要是包长），
/// 然后根据 包长从 base 中读取 指定长度的信息（然后在 poll_read 中复制到buf 中），
///
/// 而 Write 是 从 base 给出的 buf 中，截取 Info 中指定的长度，写入 base.
///
/// 如果Info 中指定的长度是大于给出的 buf 的，则只能添加 padding
///
/// 这里还要看时序，如果没到该 read/write 的时机，就要等待。
pub struct InfoEmbedConn {
    pub file: Arc<Vec<PayloadInfo>>,
    base: Pin<ruci::net::Conn>,
    current_packet_index: Arc<AtomicUsize>,
    behavior: ProxyBehavior,
    first_buf_to_write: Option<BytesMut>,
}

impl Display for InfoEmbedConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "info_embedder")
    }
}

impl AsyncRead for InfoEmbedConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.behavior {
            ProxyBehavior::UNSPECIFIED => panic!("shoudn't happen"),
            ProxyBehavior::ENCODE => todo!(),
            ProxyBehavior::DECODE => {
                //server
                let r = self.base.as_mut().poll_read(cx, buf);
                if let Poll::Ready(Ok(())) = &r {}
                r
            }
        }
    }
}

impl AsyncWrite for InfoEmbedConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.behavior {
            ProxyBehavior::UNSPECIFIED => panic!("shoudn't happen"),
            ProxyBehavior::ENCODE => {
                //client

                let r = self.base.as_mut().poll_write(cx, buf);

                if let Poll::Ready(Ok(u)) = &r {}
                r
            }
            ProxyBehavior::DECODE => todo!(),
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        self.base.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        self.base.as_mut().poll_shutdown(cx)
    }
}
