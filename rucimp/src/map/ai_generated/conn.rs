use super::{AIGeneratedMap, AIResult, ReadSequence, WriteSequence};
use anyhow::Result;
use futures::future::BoxFuture;
use futures_lite::FutureExt;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{ready, Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};
use tracing;

/// 连接状态
pub enum ConnState {
    Ready,
    Writing(WriteSequence),
    Reading(ReadSequence),
    ProcessingAI {
        future: Arc<Mutex<BoxFuture<'static, Result<AIResult>>>>,
        is_write: bool, // true 表示是写操作引起的，false 表示是读操作引起的
    },
    ProcessingDecodingAI {
        future: Arc<Mutex<BoxFuture<'static, Result<Vec<u8>>>>>,
    },
}

impl std::fmt::Debug for ConnState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnState::Ready => write!(f, "Ready"),
            ConnState::Writing(seq) => f
                .debug_tuple("Writing")
                .field(&seq.write_packets.len())
                .field(&seq.read_lengths.len())
                .finish(),
            ConnState::Reading(seq) => f
                .debug_tuple("Reading")
                .field(&seq.read_lengths.len())
                .field(&seq.write_packets.len())
                .finish(),
            ConnState::ProcessingAI { .. } => f.debug_tuple("ProcessingAI").finish(),
            ConnState::ProcessingDecodingAI { .. } => {
                f.debug_tuple("ProcessingDecodingAI").finish()
            }
        }
    }
}

/// AI生成的协议的连接实现
pub struct AIConn {
    pub inner: ruci::net::Conn,
    pub ai_map: AIGeneratedMap,
    pub state: ConnState,
    read_waker: Option<std::task::Waker>,  // 存储读操作的 waker
    write_waker: Option<std::task::Waker>, // 存储写操作的 waker
}

impl AIConn {
    pub fn new(inner: ruci::net::Conn, ai_map: AIGeneratedMap) -> Self {
        Self {
            inner,
            ai_map,
            state: ConnState::Ready,
            read_waker: None,
            write_waker: None,
        }
    }

    /// 开始一个写序列
    ///
    /// Will change self.state to ConnState::ProcessingAI
    fn initiate_ai_write_processing(&mut self, first_data: Vec<u8>) -> Result<()> {
        let ai_map = self.ai_map.clone();
        let future = Box::pin(async move {
            ai_map
                .generate_sequence_with_ai(&first_data, None, None, false)
                .await
        });
        self.state = ConnState::ProcessingAI {
            future: Arc::new(Mutex::new(future)),
            is_write: true,
        };
        Ok(())
    }

    /// 处理读取到的数据，可能开始新的读序列
    ///
    /// will call self.ai_map.process_with_ai
    ///
    /// will change self.state to ConnState::ProcessingAI
    fn initiate_ai_read_processing(&mut self, data: Vec<u8>) -> Result<()> {
        let ai_map = self.ai_map.clone();
        let future = Box::pin(async move {
            ai_map
                .generate_sequence_with_ai(&data, None, None, false)
                .await
        });
        self.state = ConnState::ProcessingAI {
            future: Arc::new(Mutex::new(future)),
            is_write: false,
        };
        Ok(())
    }

    // 当一个操作完成时，唤醒另一个被阻塞的操作
    fn wake_pending_operation(&mut self, completed_write: bool) {
        if completed_write {
            // 写操作完成，唤醒等待的读操作
            if let Some(waker) = self.read_waker.take() {
                waker.wake();
            }
        } else {
            // 读操作完成，唤醒等待的写操作
            if let Some(waker) = self.write_waker.take() {
                waker.wake();
            }
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
            tracing::debug!("AIConn::poll_read state: {:?}", this.state);
            match &mut this.state {
                ConnState::Ready => {
                    // 准备一个临时缓冲区
                    let mut temp_vec = vec![0u8; buf.remaining()];
                    let mut temp_buf = tokio::io::ReadBuf::new(&mut temp_vec);

                    // 从内部连接读取数据
                    let result = ready!(Pin::new(&mut this.inner).poll_read(cx, &mut temp_buf));
                    match result {
                        Ok(()) => {
                            let filled_len = temp_buf.filled().len();
                            tracing::debug!(
                                "AIConn::poll_read read {} bytes from inner",
                                filled_len
                            );
                            if filled_len > 0 {
                                temp_vec.truncate(filled_len);
                                if let Err(e) = this.initiate_ai_read_processing(temp_vec) {
                                    tracing::debug!(
                                        "AIConn::poll_read handle_read_data error: {}",
                                        e
                                    );
                                    return Poll::Ready(Err(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        e.to_string(),
                                    )));
                                }
                                continue;
                            }
                            return Poll::Ready(Ok(()));
                        }
                        Err(e) => {
                            tracing::debug!("AIConn::poll_read inner read error: {}", e);
                            return Poll::Ready(Err(e));
                        }
                    }
                }
                ConnState::ProcessingAI {
                    ref future,
                    is_write,
                } => {
                    if *is_write {
                        // 如果是写操作触发的AI处理，直接返回Pending
                        tracing::debug!("AIConn::poll_read pending due to ongoing write operation");
                        this.read_waker = Some(cx.waker().clone());
                        return Poll::Pending;
                    }

                    let mut future = future.lock().unwrap();
                    let poll_result = Pin::new(&mut *future).poll(cx);
                    drop(future);

                    match ready!(poll_result) {
                        Ok(AIResult::Read { sequence, .. }) => {
                            tracing::debug!(
                                "AIConn::poll_read AI processing completed with read sequence"
                            );
                            this.state = ConnState::Reading(sequence);
                            continue;
                        }
                        Ok(AIResult::Write(_)) => {
                            tracing::debug!("AIConn::poll_read unexpected write sequence");
                            return Poll::Ready(Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "Unexpected write sequence in read operation",
                            )));
                        }
                        Err(e) => {
                            tracing::debug!("AIConn::poll_read AI processing failed: {}", e);
                            this.state = ConnState::Ready;
                            return Poll::Ready(Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("AI processing failed: {}", e),
                            )));
                        }
                    }
                }
                ConnState::Writing(_) => {
                    tracing::debug!("AIConn::poll_read pending due to ongoing write operation");

                    // 保存 waker，等写操作完成时再唤醒
                    this.read_waker = Some(cx.waker().clone());
                    return Poll::Pending;
                }
                ConnState::Reading(sequence) => {
                    if sequence.current_step.is_read {
                        // 执行读子序列 (r*)
                        if sequence.current_step.index < sequence.read_lengths.len() {
                            let expected_len = sequence.read_lengths[sequence.current_step.index];
                            tracing::debug!(
                                "AIConn::poll_read reading r{} with length {}",
                                sequence.current_step.index,
                                expected_len
                            );
                            let mut temp_vec = vec![0u8; expected_len];
                            let mut temp_buf = tokio::io::ReadBuf::new(&mut temp_vec);

                            match ready!(Pin::new(&mut this.inner).poll_read(cx, &mut temp_buf)) {
                                Ok(()) => {
                                    let filled_len = temp_buf.filled().len();
                                    tracing::debug!(
                                        "AIConn::poll_read r{} completed with {} bytes",
                                        sequence.current_step.index,
                                        filled_len
                                    );
                                    // 根据实际填充的长度截取 temp_vec
                                    temp_vec.truncate(filled_len);
                                    sequence.read_packets.push(temp_vec);

                                    // 切换到写操作
                                    sequence.advance_to_next_write();
                                    continue;
                                }
                                Err(e) => {
                                    tracing::debug!(
                                        "AIConn::poll_read r{} failed: {}",
                                        sequence.current_step.index,
                                        e
                                    );
                                    return Poll::Ready(Err(e));
                                }
                            }
                        } else {
                            // 读取序列完成
                            tracing::debug!("AIConn::poll_read read sequence completed");

                            // 合并所有读取的数据包
                            let mut combined_data = Vec::new();
                            combined_data.extend(sequence.read_packets.drain(..).flatten());

                            // 创建解密future
                            let ai_map = this.ai_map.clone();
                            let future = Box::pin(async move {
                                ai_map.decrypt_read_sequence(combined_data).await
                            });

                            this.state = ConnState::ProcessingDecodingAI {
                                future: Arc::new(Mutex::new(future)),
                            };
                            continue;
                        }
                    } else {
                        // 执行写子序列 (w*)
                        if let Some(write_packet) =
                            sequence.write_packets.get(sequence.current_step.index)
                        {
                            tracing::debug!(
                                "AIConn::poll_read writing w{} with length {}",
                                sequence.current_step.index,
                                write_packet.len()
                            );
                            match ready!(Pin::new(&mut this.inner).poll_write(cx, write_packet)) {
                                Ok(_) => {
                                    tracing::debug!(
                                        "AIConn::poll_read w{} completed",
                                        sequence.current_step.index
                                    );
                                    // 切换到下一个读操作
                                    sequence.advance_to_next_read();
                                    continue;
                                }
                                Err(e) => {
                                    tracing::debug!(
                                        "AIConn::poll_read w{} failed: {}",
                                        sequence.current_step.index,
                                        e
                                    );
                                    return Poll::Ready(Err(e));
                                }
                            }
                        } else {
                            panic!("this can't happen");
                        }
                    }
                }

                ConnState::ProcessingDecodingAI { ref future } => {
                    let poll_result = Pin::new(&mut future.lock().unwrap()).poll(cx);
                    match ready!(poll_result) {
                        Ok(decrypted_data) => {
                            let len = decrypted_data.len().min(buf.remaining());
                            tracing::debug!(
                                "AIConn::poll_read returning {} decrypted bytes to caller",
                                len
                            );
                            buf.put_slice(&decrypted_data[..len]);
                            this.state = ConnState::Ready;
                            // 通知等待的写操作
                            this.wake_pending_operation(false);
                            return Poll::Ready(Ok(()));
                        }
                        Err(e) => {
                            tracing::debug!("AIConn::poll_read decryption failed: {}", e);
                            this.state = ConnState::Ready;
                            return Poll::Ready(Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("Failed to decrypt data: {}", e),
                            )));
                        }
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
        let this = &mut *self;

        loop {
            tracing::debug!("AIConn::poll_write state: {:?}", this.state);
            match &mut this.state {
                ConnState::Ready => {
                    tracing::debug!(
                        "AIConn::poll_write starting new write sequence with {} bytes",
                        buf.len()
                    );
                    if let Err(e) = this.initiate_ai_write_processing(buf.to_vec()) {
                        tracing::debug!("AIConn::poll_write start_write_sequence failed: {}", e);
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            e.to_string(),
                        )));
                    }
                    continue;
                }
                ConnState::ProcessingAI {
                    ref future,
                    is_write,
                } => {
                    if !*is_write {
                        // 如果是读操作触发的AI处理，直接返回Pending
                        tracing::debug!("AIConn::poll_write pending due to ongoing read operation");
                        this.write_waker = Some(cx.waker().clone());
                        return Poll::Pending;
                    }

                    let mut future = future.lock().unwrap();
                    let poll_result = Pin::new(&mut *future).poll(cx);
                    drop(future);

                    match ready!(poll_result) {
                        Ok(AIResult::Write(sequence)) => {
                            tracing::debug!(
                                "AIConn::poll_write AI processing completed with write sequence"
                            );
                            this.state = ConnState::Writing(sequence);
                            continue;
                        }
                        Ok(AIResult::Read { .. }) => {
                            tracing::debug!("AIConn::poll_write unexpected read sequence");
                            return Poll::Ready(Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "Unexpected read sequence in write operation",
                            )));
                        }
                        Err(e) => {
                            tracing::debug!("AIConn::poll_write AI processing failed: {}", e);
                            return Poll::Ready(Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("AI processing failed: {}", e),
                            )));
                        }
                    }
                }
                ConnState::Reading(_) | ConnState::ProcessingDecodingAI { .. } => {
                    tracing::debug!("AIConn::poll_write pending due to ongoing read operation");

                    // 保存 waker，等读操作完成时再唤醒
                    this.write_waker = Some(cx.waker().clone());
                    return Poll::Pending;
                }
                ConnState::Writing(sequence) => {
                    if sequence.current_step.is_write {
                        // 执行写操作 (w*)
                        if let Some(write_packet) =
                            sequence.write_packets.get(sequence.current_step.index)
                        {
                            tracing::debug!(
                                "AIConn::poll_write writing w{} with length {}",
                                sequence.current_step.index,
                                write_packet.len()
                            );
                            match ready!(Pin::new(&mut this.inner).poll_write(cx, write_packet)) {
                                Ok(_) => {
                                    tracing::debug!(
                                        "AIConn::poll_write w{} completed",
                                        sequence.current_step.index
                                    );
                                    // 切换到读操作
                                    sequence.advance_to_next_read();
                                    continue;
                                }
                                Err(e) => {
                                    tracing::debug!(
                                        "AIConn::poll_write w{} failed: {}",
                                        sequence.current_step.index,
                                        e
                                    );
                                    return Poll::Ready(Err(e));
                                }
                            }
                        } else {
                            panic!("this can't happen");
                        }
                    } else {
                        // 执行读操作 (r*)
                        if let Some(&read_len) =
                            sequence.read_lengths.get(sequence.current_step.index)
                        {
                            if read_len > 0 {
                                tracing::debug!(
                                    "AIConn::poll_write reading r{} with length {}",
                                    sequence.current_step.index,
                                    read_len
                                );
                                let mut temp_vec = vec![0u8; read_len];
                                let mut temp_buf = tokio::io::ReadBuf::new(&mut temp_vec);

                                match ready!(Pin::new(&mut this.inner).poll_read(cx, &mut temp_buf))
                                {
                                    Ok(()) => {
                                        let filled_len = temp_buf.filled().len();
                                        temp_vec.truncate(filled_len);
                                        tracing::debug!(
                                            "AIConn::poll_write r{} completed",
                                            sequence.current_step.index
                                        );

                                        if sequence.current_step.index + 1
                                            >= sequence.read_lengths.len()
                                        {
                                            // 序列完成
                                            tracing::debug!("AIConn::poll_write sequence completed (at final read)");
                                            this.state = ConnState::Ready;
                                            return Poll::Ready(Ok(buf.len()));
                                        } else {
                                            // 切换到下一个写操作
                                            sequence.advance_to_next_write();
                                            continue;
                                        }
                                    }
                                    Err(e) => {
                                        tracing::debug!(
                                            "AIConn::poll_write r{} failed: {}",
                                            sequence.current_step.index,
                                            e
                                        );
                                        return Poll::Ready(Err(e));
                                    }
                                }
                            } else {
                                // 空读操作，直接切换到下一个写操作
                                tracing::debug!(
                                    "AIConn::poll_write skipping empty r{}",
                                    sequence.current_step.index
                                );
                                sequence.advance_to_next_write();
                                continue;
                            }
                        } else {
                            panic!("this can't happen");
                        }
                    }
                }
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        tracing::debug!("AIConn::poll_flush");
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        tracing::debug!("AIConn::poll_shutdown");
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
