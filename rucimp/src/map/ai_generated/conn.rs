use super::AIGeneratedMap;
use anyhow::Result;
use futures::future::BoxFuture;
use ruci::net;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};

/// AI生成的协议的连接实现
pub struct AIConn {
    inner: ruci::net::Conn,
    ai_map: AIGeneratedMap,
    first_packet: Option<Vec<u8>>, // 第一个数据包
    is_first_write: bool,
    read_state: ReadState,
}

/// 读取状态
enum ReadState {
    Ready,
    Processing {
        future: Arc<Mutex<BoxFuture<'static, Result<(Vec<u8>, Option<net::Addr>)>>>>,
    },
}

impl AIConn {
    pub fn new(inner: ruci::net::Conn, ai_map: AIGeneratedMap, first_packet: Vec<u8>) -> Self {
        Self {
            inner,
            ai_map,
            first_packet: Some(first_packet),
            is_first_write: true,
            read_state: ReadState::Ready,
        }
    }
}

impl AsyncRead for AIConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = &mut *self;

        loop {
            match &mut this.read_state {
                ReadState::Ready => {
                    // 准备一个临时缓冲区
                    let mut temp_vec = vec![0u8; buf.remaining()];
                    let mut temp_buf = tokio::io::ReadBuf::new(temp_vec.as_mut_slice());

                    // 从内部连接读取数据
                    match Pin::new(&mut this.inner).poll_read(cx, &mut temp_buf) {
                        Poll::Ready(Ok(())) => {
                            let filled_data = temp_buf.filled().to_vec();
                            if !filled_data.is_empty() {
                                // 直接创建 process_with_ai 的 future
                                let filled_data2 = filled_data.clone();
                                let ai_map = this.ai_map.clone();
                                let future: Arc<
                                    Mutex<BoxFuture<'static, Result<(Vec<u8>, Option<net::Addr>)>>>,
                                > = Arc::new(Mutex::new(Box::pin(async move {
                                    ai_map
                                        .process_with_ai(&filled_data2, None, None, false)
                                        .await
                                })));

                                // 更新状态为处理中
                                this.read_state = ReadState::Processing { future };
                                continue;
                            } else {
                                return Poll::Ready(Ok(()));
                            }
                        }
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    }
                }
                ReadState::Processing { ref future, .. } => {
                    let mut future_lock = future.lock().unwrap();
                    let poll_result = future_lock.as_mut().poll(cx);
                    drop(future_lock); // 释放锁

                    match poll_result {
                        Poll::Ready(Ok((processed_data, _))) => {
                            // 将处理后的数据写入缓冲区
                            let len = processed_data.len().min(buf.remaining());
                            buf.put_slice(&processed_data[..len]);

                            // 重置状态
                            this.read_state = ReadState::Ready;
                            return Poll::Ready(Ok(()));
                        }
                        Poll::Ready(Err(e)) => {
                            // 重置状态并返回错误
                            this.read_state = ReadState::Ready;
                            return Poll::Ready(Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("Failed to process data: {}", e),
                            )));
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }
            }
        }
    }
}

impl AsyncWrite for AIConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.is_first_write {
            // 首先发送第一个数据包
            if let Some(first_packet) = self.first_packet.take() {
                match Pin::new(&mut self.inner).poll_write(cx, &first_packet) {
                    Poll::Ready(Ok(_)) => {
                        self.is_first_write = false;
                        // 继续写入当前数据
                        Pin::new(&mut self.inner).poll_write(cx, buf)
                    }
                    Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                    Poll::Pending => Poll::Pending,
                }
            } else {
                self.is_first_write = false;
                Pin::new(&mut self.inner).poll_write(cx, buf)
            }
        } else {
            Pin::new(&mut self.inner).poll_write(cx, buf)
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
