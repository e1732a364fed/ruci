use super::{AIGeneratedMap, AISequence};
use anyhow::Result;
use futures::future::BoxFuture;
use futures_lite::FutureExt;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{ready, Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};

/// 写序列状态
#[derive(Debug)]
struct WriteSequence {
    write_packets: VecDeque<Vec<u8>>,    // 待发送的数据包 w1,w2,...,wN
    read_lengths: VecDeque<usize>,       // 期望接收的数据长度 r1,r2,...,rN
    current_step: usize,                 // 当前执行到第几步
    waiting_for_read: bool,              // 是否正在等待读取响应
    expected_read_length: Option<usize>, // 期望读取的长度
}

/// 读序列状态
#[derive(Debug)]
struct ReadSequence {
    write_lengths: VecDeque<usize>,  // 需要发送的响应长度 w1,w2,...,wN
    read_packets: VecDeque<Vec<u8>>, // 期望接收的数据包 r1,r2,...,rN
    current_step: usize,             // 当前执行到第几步
    pending_write: Option<Vec<u8>>,  // 待写入的响应数据
}

/// 连接状态
enum ConnState {
    Ready,
    Writing(WriteSequence),
    Reading(ReadSequence),
    ProcessingAI {
        future: Arc<Mutex<BoxFuture<'static, Result<AISequence>>>>,
    },
}

/// AI生成的协议的连接实现
pub struct AIConn {
    inner: ruci::net::Conn,
    ai_map: AIGeneratedMap,
    first_packet: Option<Vec<u8>>,
    state: ConnState,
}

impl AIConn {
    pub fn new(inner: ruci::net::Conn, ai_map: AIGeneratedMap, first_sequence: AISequence) -> Self {
        Self {
            inner,
            ai_map,
            first_packet: Some(
                first_sequence
                    .write_packets
                    .into_iter()
                    .next()
                    .unwrap_or_default(),
            ),
            state: ConnState::Ready,
        }
    }

    /// 开始一个写序列
    ///
    /// Will change self.state to ConnState::ProcessingAI
    fn start_write_sequence(&mut self, first_data: Vec<u8>) -> Result<()> {
        let ai_map = self.ai_map.clone();
        let future =
            Box::pin(async move { ai_map.process_with_ai(&first_data, None, None, false).await });
        self.state = ConnState::ProcessingAI {
            future: Arc::new(Mutex::new(future)),
        };
        Ok(())
    }

    /// 从AI返回的序列信息创建写序列
    ///
    /// Will change self.state to ConnState::Writing
    fn create_write_sequence(&mut self, sequence: AISequence) -> Result<()> {
        let write_sequence = WriteSequence {
            write_packets: VecDeque::from(sequence.write_packets),
            read_lengths: VecDeque::from(sequence.read_lengths),
            current_step: 0,
            waiting_for_read: false,
            expected_read_length: None,
        };
        self.state = ConnState::Writing(write_sequence);
        Ok(())
    }

    /// 处理读取到的数据，可能开始新的读序列
    fn handle_read_data(&mut self, data: Vec<u8>) -> Result<()> {
        let ai_map = self.ai_map.clone();
        let future =
            Box::pin(async move { ai_map.process_with_ai(&data, None, None, false).await });
        self.state = ConnState::ProcessingAI {
            future: Arc::new(Mutex::new(future)),
        };
        Ok(())
    }

    /// 从AI返回的序列信息创建读序列
    ///
    /// Will change self.state to ConnState::Reading
    fn create_read_sequence(&mut self, sequence: AISequence) -> Result<()> {
        let read_sequence = ReadSequence {
            write_lengths: VecDeque::from(
                sequence
                    .write_packets
                    .iter()
                    .map(|p| p.len())
                    .collect::<Vec<_>>(),
            ),
            read_packets: VecDeque::from(sequence.write_packets),
            current_step: 0,
            pending_write: None,
        };
        self.state = ConnState::Reading(read_sequence);
        Ok(())
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
            match &mut this.state {
                ConnState::Ready => {
                    // 准备一个临时缓冲区
                    let mut temp_vec = vec![0u8; buf.remaining()];
                    let mut temp_buf = tokio::io::ReadBuf::new(&mut temp_vec);

                    // 从内部连接读取数据
                    let result = ready!(Pin::new(&mut this.inner).poll_read(cx, &mut temp_buf));
                    match result {
                        Ok(()) => {
                            let filled_data = temp_buf.filled().to_vec();
                            if !filled_data.is_empty() {
                                if let Err(e) = this.handle_read_data(filled_data) {
                                    return Poll::Ready(Err(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        e.to_string(),
                                    )));
                                }
                                continue;
                            }
                            return Poll::Ready(Ok(()));
                        }
                        Err(e) => return Poll::Ready(Err(e)),
                    }
                }
                ConnState::ProcessingAI { ref future } => {
                    let mut future = future.lock().unwrap();
                    let poll_result = Pin::new(&mut *future).poll(cx);
                    drop(future);

                    let sequence = match ready!(poll_result) {
                        Ok(sequence) => sequence,
                        Err(e) => {
                            this.state = ConnState::Ready;
                            return Poll::Ready(Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("AI processing failed: {}", e),
                            )));
                        }
                    };

                    if let Err(e) = this.create_read_sequence(sequence) {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            e.to_string(),
                        )));
                    }
                    continue;
                }
                ConnState::Reading(sequence) => {
                    // 如果有待写入的响应
                    if let Some(data) = sequence.pending_write.take() {
                        match Pin::new(&mut this.inner).poll_write(cx, &data) {
                            Poll::Ready(Ok(_)) => {
                                sequence.current_step += 1;
                                continue;
                            }
                            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                            Poll::Pending => {
                                sequence.pending_write = Some(data);
                                return Poll::Pending;
                            }
                        }
                    }

                    // 获取期望的读取数据
                    if let Some(expected_data) = sequence.read_packets.front() {
                        let len = expected_data.len().min(buf.remaining());
                        buf.put_slice(&expected_data[..len]);
                        sequence.read_packets.pop_front();
                        sequence.current_step += 1;

                        // 检查是否需要准备写入响应
                        if let Some(&write_len) = sequence.write_lengths.front() {
                            if write_len > 0 {
                                // TODO: 生成响应数据
                                sequence.pending_write = Some(vec![0; write_len]);
                            }
                            sequence.write_lengths.pop_front();
                        }

                        // 如果序列完成，重置状态
                        if sequence.read_packets.is_empty() && sequence.pending_write.is_none() {
                            this.state = ConnState::Ready;
                        }
                        return Poll::Ready(Ok(()));
                    } else {
                        this.state = ConnState::Ready;
                        continue;
                    }
                }
                ConnState::Writing(sequence) => {
                    if sequence.waiting_for_read {
                        // 如果正在等待读取响应，尝试读取
                        let expected_len = sequence.expected_read_length.unwrap_or(0);
                        if expected_len > 0 {
                            let mut temp_vec = vec![0u8; expected_len];
                            let mut temp_buf = tokio::io::ReadBuf::new(&mut temp_vec);

                            ready!(Pin::new(&mut this.inner).poll_read(cx, &mut temp_buf))?;
                        }
                        sequence.waiting_for_read = false;
                        sequence.expected_read_length = None;
                    }
                    continue;
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

        // 首先处理首包（如果有）
        if let Some(first_packet) = this.first_packet.take() {
            match Pin::new(&mut this.inner).poll_write(cx, &first_packet) {
                Poll::Ready(Ok(_)) => {
                    // 继续处理当前数据
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => {
                    this.first_packet = Some(first_packet);
                    return Poll::Pending;
                }
            }
        }

        match &mut this.state {
            ConnState::Ready => {
                // 开始新的写序列
                if let Err(e) = this.start_write_sequence(buf.to_vec()) {
                    return Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )));
                }
                Poll::Ready(Ok(buf.len()))
            }
            ConnState::ProcessingAI { ref future } => {
                let mut future = future.lock().unwrap();
                let poll_result = Pin::new(&mut *future).poll(cx);
                drop(future);

                match ready!(poll_result) {
                    Ok(sequence) => {
                        if let Err(e) = this.create_write_sequence(sequence) {
                            return Poll::Ready(Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                e.to_string(),
                            )));
                        }
                        Poll::Ready(Ok(buf.len()))
                    }
                    Err(e) => Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("AI processing failed: {}", e),
                    ))),
                }
            }
            ConnState::Writing(sequence) => {
                if sequence.waiting_for_read {
                    // 如果正在等待读取响应，返回 Pending
                    return Poll::Pending;
                }

                if let Some(packet) = sequence.write_packets.front() {
                    match Pin::new(&mut this.inner).poll_write(cx, packet) {
                        Poll::Ready(Ok(_)) => {
                            sequence.write_packets.pop_front();
                            sequence.current_step += 1;

                            // 检查是否需要等待读取响应
                            if let Some(&read_len) = sequence.read_lengths.front() {
                                if read_len > 0 {
                                    sequence.waiting_for_read = true;
                                    sequence.expected_read_length = Some(read_len);
                                }
                                sequence.read_lengths.pop_front();
                            }

                            // 如果序列完成，重置状态
                            if sequence.write_packets.is_empty() && !sequence.waiting_for_read {
                                this.state = ConnState::Ready;
                            }
                            Poll::Ready(Ok(buf.len()))
                        }
                        Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                        Poll::Pending => Poll::Pending,
                    }
                } else {
                    this.state = ConnState::Ready;
                    Poll::Ready(Ok(buf.len()))
                }
            }
            ConnState::Reading(_) => Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Cannot write while in read sequence",
            ))),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
