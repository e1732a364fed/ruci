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

impl Embedder {
    pub fn new(file_content: Vec<u8>, file_name: String) -> anyhow::Result<Self> {
        let ext = file_name
            .split('.')
            .last()
            .unwrap_or_default()
            .to_lowercase();
        let extension = ext.as_str();

        let extension = match extension {
            "json" => crate::map::recorder::OutputFileExtension::Json,
            "cbor" => crate::map::recorder::OutputFileExtension::Cbor,
            _ => anyhow::bail!("invalid file extension: {}", extension),
        };

        let info_data = crate::map::recorder::InfoData::new(file_content, extension)?;
        Ok(Self {
            file: Arc::new(info_data.payload),
            ext_fields: Default::default(),
        })
    }
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

        let mut conn = EmbedConn {
            file: self.file.clone(),
            base: Box::pin(c),
            current_packet_index: Arc::new(AtomicUsize::new(0)),
            behavior,
            write_state: None,
            read_state: Default::default(),
            read_waker: None,
            write_waker: None,
        };

        match behavior {
            ProxyBehavior::ENCODE => {
                let b = ruci::utils::ob_to_buf(ob);
                if !b.is_empty() {
                    if self.is_tail_of_chain() {
                        conn.write_state = Some(WriteState::FirstBufToWrite(b));

                        return MapResult::new_c(Box::new(conn)).a(params.a).build();
                    }
                }
                let ob = ruci::utils::buf_to_ob(b);

                MapResult::new_c(Box::new(conn)).b(ob).a(params.a).build()
            }
            ProxyBehavior::DECODE => {
                conn.read_state.cur_info_read_buf = ruci::utils::ob_to_buf(ob);
                //第一个包被认为是完全的

                MapResult::new_c(Box::new(conn)).a(params.a).build()
            }
            ProxyBehavior::UNSPECIFIED => panic!("shoudn't happen"),
        }
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
    read_state: ReadState,

    read_waker: Option<std::task::Waker>,  // 存储读操作的 waker
    write_waker: Option<std::task::Waker>, // 存储写操作的 waker
}

#[derive(Default)]
struct ReadState {
    content_len: Option<usize>,
    content_read_buf_filled_state: FilledState,
    cur_info_read_buf: BytesMut,
    content_read_buf: BytesMut,
}
#[derive(Default)]
enum FilledState {
    Full,
    Partial,
    #[default]
    None,
    Parse,
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
    // 唤醒一个被阻塞的操作
    fn wake_pending_operation(&mut self, is_write: bool) {
        if is_write {
            //唤醒等待的写操作
            if let Some(waker) = self.write_waker.take() {
                debug!("embed wake write");
                waker.wake();
            }
        } else {
            //唤醒等待的读操作
            if let Some(waker) = self.read_waker.take() {
                debug!("embed wake read");

                waker.wake();
            }
        }
    }

    fn advance_packet_index(&mut self) {
        let idx = self
            .current_packet_index
            .load(std::sync::atomic::Ordering::Relaxed);

        let cur_idx = if idx + 1 == self.file.len() {
            self.current_packet_index
                .store(0, std::sync::atomic::Ordering::Relaxed);
            0
        } else {
            self.current_packet_index
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        };

        let vec = self.file.clone();
        let cur_info = vec.get(cur_idx).unwrap();

        if direction_match_write(self.behavior, cur_info.direction) {
            self.wake_pending_operation(true)
        } else {
            self.wake_pending_operation(false)
        }
    }

    /// read to cur_info_read_buf
    fn read_from_base(
        &mut self,
        cx: &mut std::task::Context<'_>,
        info_len: usize,
    ) -> Poll<io::Result<usize>> {
        let old_len = self.read_state.cur_info_read_buf.len();

        let mut rbuf = {
            self.read_state.cur_info_read_buf.resize(info_len, 0);
            ReadBuf::new(&mut self.read_state.cur_info_read_buf[old_len..])
        };
        let r = self.base.as_mut().poll_read(cx, &mut rbuf);

        let fl = rbuf.filled().len();

        self.read_state.cur_info_read_buf.resize(fl + old_len, 0);

        Poll::Ready(match ready!(r) {
            Ok(()) => Ok(fl),
            Err(e) => Err(e),
        })
    }

    fn reserve_cur_info_read_buf(&mut self) {
        let buf = &mut self.read_state.cur_info_read_buf;
        if buf.capacity() < CAP {
            let additional = CAP - buf.capacity();
            buf.reserve(additional);
        }
    }

    fn cur_info_read_buf(&mut self) -> &mut BytesMut {
        &mut self.read_state.cur_info_read_buf
    }
}

impl ReadState {
    fn put_cur_info_read_buf_to_content_buf(&mut self, cur_info_read_buf_len: usize) {
        let rm = &self.cur_info_read_buf[..cur_info_read_buf_len];
        self.content_read_buf.put_slice(rm);
    }
}

const CAP: usize = 1024 * 1024;

impl AsyncRead for EmbedConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        rbuf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            let current_packet_index = self
                .current_packet_index
                .load(std::sync::atomic::Ordering::Relaxed);

            let vec = self.file.clone();
            let cur_info = vec.get(current_packet_index).unwrap();

            debug!("EmbedConn::poll_read: cur_info: {:?}", cur_info);

            if direction_match_write(self.behavior, cur_info.direction) {
                debug!("read pending {:?} {}", self.behavior, cur_info.direction);
                self.read_waker = Some(cx.waker().clone());
                return Poll::Pending;
            }

            let info_len = cur_info.length;

            match self.read_state.content_len {
                Some(content_len) => {
                    match self.read_state.content_read_buf_filled_state {
                        FilledState::Full => {
                            // 此时仅剩一个任务，就是将 content_read_buf 中的剩余内容 复制到 rbuf中

                            let len_to_put =
                                rbuf.remaining().min(self.read_state.content_read_buf.len());
                            rbuf.put_slice(&self.read_state.content_read_buf[..len_to_put]);

                            self.read_state.content_read_buf.advance(len_to_put);

                            if self.read_state.content_read_buf.is_empty() {
                                self.read_state.content_len = None;
                            }

                            return Poll::Ready(Ok(()));
                        }
                        FilledState::Partial => {
                            //此时要继续读取 remote 内容 并 解包、添充到 content_read_buf

                            let r = self.read_from_base(cx, info_len);
                            match ready!(r) {
                                Err(e) => return Poll::Ready(Err(e)),

                                Ok(n) => {
                                    if self.read_state.cur_info_read_buf.len() < info_len {
                                        debug!("n < info_len ");
                                    } else {
                                        debug_assert_eq!(
                                            self.read_state.cur_info_read_buf.len(),
                                            info_len
                                        );

                                        let cbuflen = self.read_state.content_read_buf.len();
                                        let need = content_len - cbuflen;

                                        if n < need {
                                            self.read_state.put_cur_info_read_buf_to_content_buf(n);
                                        } else {
                                            self.read_state
                                                .put_cur_info_read_buf_to_content_buf(need);
                                            self.read_state.content_read_buf_filled_state =
                                                FilledState::Full;
                                        }
                                    }
                                    continue;
                                }
                            }
                        }

                        FilledState::None => {
                            //此时是一个 info 包没读完的情况

                            let r = self.read_from_base(cx, info_len);
                            match ready!(r) {
                                Err(e) => return Poll::Ready(Err(e)),

                                Ok(n) => {
                                    if n == 0 {
                                        debug!("read got EOF2");
                                        return Poll::Ready(Ok(()));
                                    }
                                    if self.cur_info_read_buf().len() == info_len {
                                        self.read_state.content_read_buf_filled_state =
                                            FilledState::Parse;
                                    }
                                    continue;
                                }
                            }
                        }
                        FilledState::Parse => {
                            let cirb = self.cur_info_read_buf();

                            let cirb_len = cirb.len();
                            debug!("parse cirb_len: {}", cirb_len);

                            if content_len <= cirb_len {
                                let remaining = rbuf.remaining();
                                if remaining >= cirb_len {
                                    // 最好的情况
                                    rbuf.put_slice(&cirb[..content_len]);

                                    cirb.clear();

                                    self.advance_packet_index();
                                    self.read_state.content_len = None;

                                    debug!("read parse ok");
                                    return Poll::Ready(Ok(()));
                                } else {
                                    // 给的 rbuf 太短 的情况

                                    rbuf.put_slice(&cirb[..remaining]);

                                    self.read_state.content_read_buf_filled_state =
                                        FilledState::Full;

                                    self.read_state.content_read_buf =
                                        self.read_state.cur_info_read_buf.split_to(remaining);

                                    self.read_state.content_len = Some(content_len);

                                    continue;
                                }
                            } else {
                                // content_len 超过了 一个 info 包的长度 的情况

                                self.read_state.content_len = Some(content_len);
                                self.read_state.content_read_buf_filled_state =
                                    FilledState::Partial;
                                self.advance_packet_index();
                                self.read_state
                                    .put_cur_info_read_buf_to_content_buf(cirb_len);
                                self.read_state.cur_info_read_buf.clear();

                                continue;
                            }
                        }
                    }
                }
                None => {
                    self.cur_info_read_buf().clear();
                    self.reserve_cur_info_read_buf();

                    //此时要读取包，得到第一个 info 中的 包头,每个包头为2字节,(即内容最大长度为64k)
                    let r = self.read_from_base(cx, info_len);
                    match ready!(r) {
                        Err(e) => return Poll::Ready(Err(e)),

                        Ok(n) => {
                            if n == 0 {
                                //EOF
                                debug!("read got EOF");
                                return Poll::Ready(Ok(()));
                            }
                            if n < 2 {
                                self.cur_info_read_buf().clear();
                                debug!("read got n < 2 ");
                                continue;
                            }
                            let cl = self.cur_info_read_buf().get_u16() as usize;

                            debug!("read got cl: {}", cl);

                            self.read_state.content_len = Some(cl);

                            if n < info_len {
                                // 一个 info 包 没有 读完整 的情况

                                self.read_state.content_read_buf_filled_state = FilledState::None;
                                debug!("read got n < info_len ");
                            } else {
                                debug_assert_eq!(n, info_len);

                                debug!("read got n == info_len ");

                                self.read_state.content_read_buf_filled_state = FilledState::Parse;
                            }

                            continue;
                        } //Ok(n)
                    } // match ready!
                } //None
            } //match content_len
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
        debug!(
            "want to write buf len: {}, {}",
            buf.len(),
            buf.escape_ascii()
        );
        // 太小了就直接写入 padding 包
        if cur_info.length < 2 {
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

        let actual_allowed_data_len = cur_info.length - 2;

        // 写时，分两种情况
        // 如果是 buf.len() <= cur_info.length - 2, 则 直接 加一个 2字节的包头 后写入, 且加上一个 padding, 使
        // 实际写入的长度 正好等于 cur_info.length，

        // 而如果不满足上面条件，则 只 截取 buf 中 长度为 cur_info.length - 2 的数据， 然后 加上一个 2字节的包头 后写入

        let len_to_write_after_head = actual_allowed_data_len.min(buf.len());

        let mut fitted_buf = BytesMut::with_capacity(cur_info.length);
        fitted_buf.put_u16(len_to_write_after_head as u16);
        fitted_buf.put_slice(&buf[..len_to_write_after_head]);

        if actual_allowed_data_len > buf.len() {
            fitted_buf.resize(cur_info.length, 0);
        }

        let r = self.base.as_mut().poll_write(cx, &fitted_buf);

        debug!("actual write len {}, r:{:?}", cur_info.length, r);

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

            debug!("EmbedConn::poll_write: cur_info: {:?}", cur_info);

            if !direction_match_write(self.behavior, cur_info.direction) {
                debug!(
                    "embed write pending {:?} {}",
                    self.behavior, cur_info.direction
                );
                self.write_waker = Some(cx.waker().clone());

                return Poll::Pending;
            }

            match self.write_state.take() {
                None => {
                    debug!("write buf when write_state is None");
                    let r = self.write_buf(cur_info, cx, buf, false);
                    match ready!(r) {
                        WriteBufResult::Continue => {
                            debug!("write continue");
                            continue;
                        }
                        WriteBufResult::Done(r) => return Poll::Ready(r),
                    }
                }
                Some(state) => {
                    match state {
                        WriteState::FirstBufToWrite(first_buf) => {
                            debug!(
                                "write first buf {}",
                                &first_buf[..50.min(first_buf.len())].escape_ascii()
                            );
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
