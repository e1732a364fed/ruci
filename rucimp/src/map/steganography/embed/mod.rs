use async_trait::async_trait;
use bytes::{Buf, BufMut, BytesMut};
use macro_map::{map_ext_fields, MapExt};
use pin_project::pin_project;
use ruci::map;
use ruci::map::*;
use ruci::net::CID;
use std::io;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::task::{ready, Poll};
use std::{fmt::Display, pin::Pin};
use tokio::io::{AsyncRead, AsyncWrite};
// use tracing::{debug, error, info};

use crate::map::recorder::{PayloadInfo, DOWNLOAD_DIRECTION, UPLOAD_DIRECTION};

fn direction_match_behavior(behavior: ProxyBehavior, direction: i8) -> bool {
    matches!(behavior, ProxyBehavior::DECODE) && direction == DOWNLOAD_DIRECTION
        || matches!(behavior, ProxyBehavior::ENCODE) && direction == UPLOAD_DIRECTION
}

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

        let mut ob = params.b;

        if ob.is_some() {
            if ob.as_ref().unwrap().is_empty() {
                ob = None;
            }
        }

        let mut info_conn = InfoEmbedConn {
            file: self.file.clone(),
            base: Box::pin(c),
            current_packet_index: Arc::new(AtomicUsize::new(0)),
            behavior,
            buf_to_write: None,
            buf_to_read: None,
        };
        match behavior {
            ProxyBehavior::ENCODE => {
                info_conn.buf_to_write = ob;
            }
            ProxyBehavior::DECODE => {
                info_conn.buf_to_read = ob;
            }
            ProxyBehavior::UNSPECIFIED => panic!("shoudn't happen"),
        }

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
#[pin_project]
pub struct InfoEmbedConn {
    pub file: Arc<Vec<PayloadInfo>>,
    base: Pin<ruci::net::Conn>,
    current_packet_index: Arc<AtomicUsize>,
    behavior: ProxyBehavior,
    buf_to_write: Option<BytesMut>,
    buf_to_read: Option<BytesMut>,
}

impl Display for InfoEmbedConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "info_embedder")
    }
}

impl InfoEmbedConn {
    fn advance_packet_index(&self) {
        let idx = self
            .current_packet_index
            .load(std::sync::atomic::Ordering::Relaxed);

        if idx + 1 == self.file.len() {
            self.current_packet_index
                .store(0, std::sync::atomic::Ordering::Relaxed);
        } else {
            self.current_packet_index
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl AsyncRead for InfoEmbedConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let current_packet_index = self
            .current_packet_index
            .load(std::sync::atomic::Ordering::Relaxed);

        let vec = self.file.clone();
        let cur_info = vec.get(current_packet_index).unwrap();

        if direction_match_behavior(self.behavior, cur_info.direction) {
            if self.buf_to_read.is_some() {
                todo!()
            } else {
                let len_to_read = cur_info.length.min(buf.remaining());

                let mut temp_buf =
                    tokio::io::ReadBuf::new(&mut buf.initialize_unfilled()[..len_to_read]);

                match ready!(self.base.as_mut().poll_read(cx, &mut temp_buf)) {
                    Ok(()) => {
                        let filled = temp_buf.filled().len();
                        buf.advance(filled);

                        if filled == cur_info.length {
                            self.advance_packet_index();
                        } else {
                        }
                        Poll::Ready(Ok(()))
                    }
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
        } else {
            Poll::Pending
        }
    }
}

impl AsyncWrite for InfoEmbedConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let current_packet_index = self
            .current_packet_index
            .load(std::sync::atomic::Ordering::Relaxed);

        let cur_info = self.file.get(current_packet_index).unwrap();

        if direction_match_behavior(self.behavior, cur_info.direction) {
            if self.buf_to_write.is_some() {
                let mut buf_to_write = self.buf_to_write.take().unwrap();

                let len_to_write = buf_to_write.len().min(buf.len());
                let r = self
                    .base
                    .as_mut()
                    .poll_write(cx, &buf_to_write[..len_to_write]);

                match r {
                    Poll::Ready(Ok(u)) => {
                        buf_to_write.advance(u);
                        if !buf_to_write.is_empty() {
                            self.buf_to_write = Some(buf_to_write);
                        } else {
                            self.advance_packet_index();
                        }

                        return Poll::Ready(Ok(u));
                    }
                    Poll::Pending => {
                        self.buf_to_write = Some(buf_to_write);
                        return Poll::Pending;
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                }
            } else {
                let len_to_write = cur_info.length.min(buf.len());
                let r = self.base.as_mut().poll_write(cx, &buf[..len_to_write]);

                match ready!(r) {
                    Ok(u) => {
                        if len_to_write == u {
                            self.advance_packet_index();
                        } else {
                            let remain_buf = BytesMut::from(&buf[u..]);
                            self.buf_to_write = Some(remain_buf);
                        }
                        return Poll::Ready(Ok(u));
                    }
                    Err(e) => return Poll::Ready(Err(e)),
                }
            }
        } else {
            Poll::Pending
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
