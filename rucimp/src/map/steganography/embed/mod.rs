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
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tracing::debug;
// use tracing::{debug, error, info};

use crate::map::recorder::{PayloadInfo, READ_DIRECTION, WRITE_DIRECTION};

fn direction_match_write(endpoint_type: ProxyBehavior, direction: i8) -> bool {
    assert!(direction.abs() == 1);
    matches!(endpoint_type, ProxyBehavior::DECODE) && direction == WRITE_DIRECTION
        || matches!(endpoint_type, ProxyBehavior::ENCODE) && direction == READ_DIRECTION
}

#[map_ext_fields]
#[derive(Debug, Clone, MapExt)]
pub struct Embedder {
    pub file: Arc<Vec<PayloadInfo>>,
}

impl Display for Embedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "embedder")
    }
}

#[async_trait]
impl Map for Embedder {
    async fn maps(&self, _cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        if matches!(behavior, ProxyBehavior::UNSPECIFIED) {
            return MapResult::from_err_str("Embedder does not support ProxyBehavior::UNSPECIFIED");
        }

        let c = params.c.try_unwrap_tcp().unwrap();

        let mut ob = params.b;

        if ob.is_some() {
            if ob.as_ref().unwrap().is_empty() {
                ob = None;
            }
        }

        let mut info_conn = EmbedConn {
            file: self.file.clone(),
            base: Box::pin(c),
            current_packet_index: Arc::new(AtomicUsize::new(0)),
            behavior,
            write_state: None,
            left_to_read_remote: 0,
            read_buf: BytesMut::new(),
            real_read_data_buf: BytesMut::new(),
            read_buf_start_index: 0,
            read_left_to_read_remote_reason: None,
        };

        match behavior {
            ProxyBehavior::ENCODE => {
                let b = ruci::utils::ob_to_buf(ob);
                if !b.is_empty() {
                    info_conn.write_state = Some(WriteState::FirstBufToWrite(b))
                }
            }
            ProxyBehavior::DECODE => {
                info_conn.read_buf = ruci::utils::ob_to_buf(ob);
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
pub struct EmbedConn {
    pub file: Arc<Vec<PayloadInfo>>,
    base: Pin<ruci::net::Conn>,
    current_packet_index: Arc<AtomicUsize>,
    behavior: ProxyBehavior,

    write_state: Option<WriteState>,

    read_left_to_read_remote_reason: Option<ReadLeftToReadRemoteReason>,

    left_to_read_remote: usize,
    read_buf: BytesMut,
    read_buf_start_index: usize,

    real_read_data_buf: BytesMut,
}

enum ReadLeftToReadRemoteReason {
    NotEnoughForContentLen,
    PacketData,
}

#[derive(Clone)]
enum WriteState {
    FirstBufToWrite(BytesMut),
    WriteRemainBuf(usize, BytesMut, bool), //the len to return, the remain buf, is_first_buf
}

impl Display for EmbedConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "info_embedder")
    }
}

impl EmbedConn {
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

    fn read_from_base(&mut self, cx: &mut std::task::Context<'_>) -> Poll<io::Result<usize>> {
        let mut new_buf = ReadBuf::new(&mut self.read_buf[self.read_buf_start_index..]);
        let r = self.base.as_mut().poll_read(cx, &mut new_buf);
        Poll::Ready(match ready!(r) {
            Ok(()) => Ok(new_buf.filled().len()),
            Err(e) => Err(e),
        })
    }
}

impl AsyncRead for EmbedConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            let current_packet_index = self
                .current_packet_index
                .load(std::sync::atomic::Ordering::Relaxed);

            let vec = self.file.clone();
            let cur_info = vec.get(current_packet_index).unwrap();

            if !direction_match_write(self.behavior, cur_info.direction) {
                return Poll::Pending;
            }

            if !self.read_buf.is_empty() {
                let remaining = buf.remaining();
                if remaining < self.read_buf.len() {
                    buf.put_slice(&self.read_buf[..remaining]);
                    self.read_buf = self.read_buf.split_off(remaining);
                    self.read_buf_start_index = 0;
                } else {
                    buf.put_slice(&self.read_buf);
                    self.read_buf.clear();
                    self.read_buf_start_index = 0;
                }

                return Poll::Ready(Ok(()));
            }

            let len_to_read = cur_info.length.min(buf.remaining());

            let is_buf_short = buf.remaining() < cur_info.length;

            // 此时 本Conn 的 read_buf 应为 空
            let cap = self.read_buf.capacity();
            if cap < cur_info.length {
                self.read_buf.reserve(cur_info.length - cap);
            }
            self.read_buf.resize(len_to_read, 0);
            self.read_buf_start_index = 0;

            // 由于本协议的原理，在 server 端读到的数据长度 应 恰好 等于 预定义的 cur_info.length

            // 如果读到的数据长度 小于 预定义的 cur_info.length，则只可能是
            // 1. 因为调用方提供的 buf 长度 小于 预定义的 cur_info.length，
            // 2. 因为网络原因，导致 读到的数据长度 小于 预定义的 cur_info.length，
            // 这两种情况下， 需要 等待 下一次 poll_read

            match ready!(self.read_from_base(cx)) {
                Ok(filled_len) => {
                    if filled_len > 0 {
                        // is_buf_short: 读满了 read_buf(buf的长度), 但是 预定义的 cur_info.length 大于 buf的长度
                        // filled_len < len_to_read: 这种情况只可能是网络因素造成，

                        // 这两种情况都需要 等待 下一次 poll_read
                        if is_buf_short || filled_len < len_to_read {
                            self.left_to_read_remote = cur_info.length - filled_len;
                            self.read_buf_start_index = filled_len;

                            self.read_left_to_read_remote_reason =
                                Some(ReadLeftToReadRemoteReason::NotEnoughForContentLen);

                            continue;
                        } else {
                            self.advance_packet_index();
                            debug!("read ok, advance packet index");

                            if self.left_to_read_remote > 0 {
                                let rl = self.read_buf.len() - self.read_buf_start_index;
                                if rl >= self.left_to_read_remote {
                                    buf.put_slice(
                                        &self.read_buf[self.read_buf_start_index
                                            ..self.read_buf_start_index + self.left_to_read_remote],
                                    );
                                    self.read_buf.clear();
                                    self.read_buf_start_index = 0;
                                    self.left_to_read_remote = 0;
                                } else {
                                    buf.put_slice(&self.read_buf[self.read_buf_start_index..]);
                                    self.read_buf.clear();
                                    self.read_buf_start_index = 0;

                                    self.left_to_read_remote -= rl;
                                }

                                return Poll::Ready(Ok(()));
                            } else {
                                if filled_len < 4 {
                                    panic!("shouldn't happen");
                                } else {
                                    let packet_len = self.read_buf.get_u32() as usize;

                                    if packet_len > self.read_buf.len() {
                                        self.left_to_read_remote = packet_len - self.read_buf.len();

                                        self.read_left_to_read_remote_reason =
                                            Some(ReadLeftToReadRemoteReason::PacketData);

                                        let remaining = buf.remaining();
                                        if remaining < self.read_buf.len() {
                                            buf.put_slice(&self.read_buf[..remaining]);
                                            self.read_buf = self.read_buf.split_off(remaining);
                                            self.read_buf_start_index = 0;
                                        } else {
                                            buf.put_slice(&self.read_buf);
                                            self.read_buf.clear();
                                            self.read_buf_start_index = 0;
                                        }

                                        return Poll::Ready(Ok(()));
                                    } else {
                                        self.left_to_read_remote = 0;
                                        buf.put_slice(&self.read_buf[..packet_len]);
                                        self.read_buf.clear();
                                        self.read_buf_start_index = 0;
                                        return Poll::Ready(Ok(()));
                                    }
                                }
                            }
                        }
                    } else {
                        // 读到 0 长度，是 base 的 EOF，这种情况就没办法了，只能直接返回
                        // if is_buf_short {
                        //     return Poll::Pending;
                        // } else {
                        debug!("read 0 length(EOF), return");
                        return Poll::Ready(Ok(()));
                        // }
                    }
                }
                Err(e) => return Poll::Ready(Err(e)),
            } // end of match
        } // end of loop
    } // end of poll_read
}

enum WriteBufResult {
    Continue,
    Done(io::Result<usize>),
}

impl EmbedConn {
    fn get_cur_info(&self) -> PayloadInfo {
        let vec = self.file.clone();
        let current_packet_index = self
            .current_packet_index
            .load(std::sync::atomic::Ordering::Relaxed);
        let cur_info = vec.get(current_packet_index).unwrap();
        cur_info.clone()
    }

    /// 其内部会在 返回前 设置好 self.write_state
    fn write_buf(
        &mut self,
        cur_info: PayloadInfo,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
        is_first_buf: bool,
    ) -> Poll<WriteBufResult> {
        // 太小了就直接写入 padding 包
        if cur_info.length < 4 {
            let mut tmp = BytesMut::with_capacity(cur_info.length);
            tmp.resize(cur_info.length, 0);
            let r = self.base.as_mut().poll_write(cx, &mut tmp);

            let r = ready!(r);
            match r {
                Ok(n) => {
                    if n != cur_info.length {
                        return Poll::Ready(WriteBufResult::Done(Err(io::Error::new(
                            io::ErrorKind::Other,
                            "write failed, n != cur_info.length",
                        ))));
                    } else {
                        self.advance_packet_index();
                        return Poll::Ready(WriteBufResult::Continue);
                    }
                }
                Err(e) => return Poll::Ready(WriteBufResult::Done(Err(e))),
            }
        }

        let actual_allowed_data_len = cur_info.length - 4;

        // 写时，分两种情况
        // 如果是 buf.len() <= cur_info.length - 4, 则 直接 加一个 4字节的包头 后写入, 且加上一个 padding, 使
        // 实际写入的长度 正好等于 cur_info.length，

        // 而如果不满足上面条件，则 只 截取 buf 中 长度为 cur_info.length - 4 的数据， 然后 加上一个 4字节的包头 后写入

        let len_to_write_after_head = actual_allowed_data_len.min(buf.len());

        let mut fitted_buf = BytesMut::with_capacity(cur_info.length);
        fitted_buf.put_u32(len_to_write_after_head as u32);
        fitted_buf.put_slice(&buf[..len_to_write_after_head]);

        if actual_allowed_data_len > buf.len() {
            fitted_buf.resize(cur_info.length, 0);
        }

        let r = self.base.as_mut().poll_write(cx, &fitted_buf);

        match ready!(r) {
            Ok(u) => {
                if u == cur_info.length {
                    self.advance_packet_index();
                    return Poll::Ready(WriteBufResult::Done(Ok(len_to_write_after_head)));
                } else {
                    // 此时只能是网络原因，需要再写一次
                    fitted_buf.advance(u);
                    self.write_state = Some(WriteState::WriteRemainBuf(
                        len_to_write_after_head,
                        fitted_buf,
                        is_first_buf,
                    ));
                    return Poll::Ready(WriteBufResult::Continue);
                }
            }
            Err(e) => {
                return Poll::Ready(WriteBufResult::Done(Err(e)));
            }
        }
    }
}

impl AsyncWrite for EmbedConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            let cur_info = self.get_cur_info();

            if !direction_match_write(self.behavior, cur_info.direction) {
                return Poll::Pending;
            }

            match self.write_state.take() {
                None => {
                    let r = self.write_buf(cur_info, cx, buf, false);
                    match ready!(r) {
                        WriteBufResult::Continue => continue,
                        WriteBufResult::Done(r) => return Poll::Ready(r),
                    }
                }
                Some(state) => {
                    match state {
                        WriteState::FirstBufToWrite(first_buf) => {
                            let r = self.write_buf(cur_info, cx, &first_buf, true);

                            let rl = first_buf.len();

                            self.write_state = Some(WriteState::FirstBufToWrite(first_buf));

                            match ready!(r) {
                                WriteBufResult::Continue => continue,
                                WriteBufResult::Done(r) => match r {
                                    Err(e) => return Poll::Ready(Err(e)),

                                    Ok(n) => {
                                        if n < rl {
                                            if let Some(WriteState::FirstBufToWrite(
                                                ref mut first_buf,
                                            )) = self.write_state.as_mut()
                                            {
                                                first_buf.advance(n);
                                            } else {
                                                panic!("shouldn't happen");
                                            }
                                        } else {
                                            self.write_state = None;
                                        }
                                        continue;
                                    }
                                },
                            }
                        }

                        WriteState::WriteRemainBuf(len_to_return, remain_buf, is_first_buf) => {
                            let r = self.base.as_mut().poll_write(cx, &remain_buf);

                            let rl = remain_buf.len();

                            self.write_state = Some(WriteState::WriteRemainBuf(
                                len_to_return,
                                remain_buf,
                                is_first_buf,
                            ));

                            match ready!(r) {
                                Err(e) => {
                                    return Poll::Ready(Err(e));
                                }
                                Ok(u) => {
                                    if u == rl {
                                        self.advance_packet_index();

                                        self.write_state = None;

                                        if is_first_buf {
                                            continue;
                                        } else {
                                            return Poll::Ready(Ok(len_to_return));
                                        }
                                    } else {
                                        // 此时只能是网络原因，需要再写一次

                                        if let Some(WriteState::WriteRemainBuf(
                                            _,
                                            ref mut remain_buf,
                                            _,
                                        )) = self.write_state.as_mut()
                                        {
                                            remain_buf.advance(u);
                                        } else {
                                            panic!("shouldn't happen");
                                        }

                                        continue;
                                    }
                                }
                            }
                        }
                    } // end of match state
                } // end of Some
            } // end of match self.write_state
        } // end of loop
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
