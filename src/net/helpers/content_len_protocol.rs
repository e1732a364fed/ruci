/*!
 * This module provides a buffered reader for protocols that have a content length in the header.
*/

use std::{io, pin::Pin, task::Poll};

use super::*;
use bytes::BytesMut;
use tokio::io::ReadBuf;

use futures::task::Context;
use tracing::trace;

#[derive(Default)]
pub enum BufferReadState {
    #[default]
    ReadyForNew,
    ContinueReadRemote {
        content_len: usize,
        body_start_index: usize,
        filled_data_len: usize,
    },
    ContinueReadLocalCache {
        from: usize,
        to: usize,
    },
    Closed,
}

pub struct PacketMetadata {
    pub content_len: usize,
    pub body_start_index: usize,
}

/// for [`BufContentLenProtocolReader`]
pub trait ParseFn: Fn(&[u8]) -> std::io::Result<PacketMetadata> + Send + Sync {}

impl<T> ParseFn for T where T: Fn(&[u8]) -> std::io::Result<PacketMetadata> + Send + Sync {}

/// Provide Buffered Reading for such protocols:
/// read the data head, and parse out a content_len,
/// the content_len may be later than the actually received data.
///
/// In that case, the read data need to be cached to get the whole data.
///
/// It needs a clousure "content_len_body_start_index_parse_fn" to be provided.
///
/// The real data must be right after the body_start_index returned by the clousure.
///
pub struct BufReader {
    read_cache: Option<BytesMut>,
    read_state: BufferReadState,
    read_cap: usize,
    reader: Pin<Box<dyn AsyncRead + Send + Sync>>,
    content_len_body_start_index_parse_fn: Box<dyn ParseFn>,
}

/// returned by [`BufContentLenProtocolReader`]'s method `read`
///
/// It should not happen that `from >= to`. So `debug_assert!(from < to);` is recommended.

pub struct BufReadResult {
    pub buf: BytesMut,
    pub body_from: usize,
    pub body_to: usize,
}

impl BufReader {
    pub fn new(
        read_cap: usize,
        reader: Pin<Box<dyn AsyncRead + Send + Sync>>,
        content_len_body_start_index_parse_fn: Box<dyn ParseFn>,
    ) -> Self {
        BufReader {
            read_cache: None,
            read_state: Default::default(),
            read_cap,
            reader,
            content_len_body_start_index_parse_fn,
        }
    }

    pub fn is_closed(&self) -> bool {
        matches!(self.read_state, BufferReadState::Closed)
    }

    /// Call it after calling [`read`] and finished using the returned buffer.
    pub fn put_back(&mut self, rc: BytesMut) {
        let _ = self.read_cache.insert(rc);
    }

    /// if read succeed, it returns the buffer, the index where the data begins
    /// and the index where the data ends, wrapped in the struct [`BufReadResult`]
    ///
    /// After reading the buffer, the user must put it back using `self.put_back(buf)`.
    ///
    /// Also, it is required that the read method will not be called again before put back.
    ///
    /// This is implemented this way to avoid extra memory copy.
    ///
    /// It's not possible that `from >= to`.
    ///
    /// Ok(None) marks EOF.
    ///
    pub fn read(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<Option<BufReadResult>>> {
        let rc = self.read_cache.take();

        match self.read_state {
            BufferReadState::Closed => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "closed",
            ))),
            BufferReadState::ContinueReadLocalCache { from, to } => {
                debug_assert!(from < to);

                let rc = rc.unwrap();

                debug_assert_eq!(rc.len(), self.read_cap);

                self.read_header(cx, rc, from, to)
            }
            BufferReadState::ReadyForNew => {
                let mut rc = rc.unwrap_or(BytesMut::zeroed(self.read_cap));

                unsafe {
                    rc.set_len(self.read_cap);
                }

                let mut rb = ReadBuf::new(&mut rc);
                match self.reader.as_mut().poll_read(cx, &mut rb) {
                    Poll::Ready(r) => match r {
                        Ok(_) => {
                            let l = rb.filled().len();
                            if l == 0 {
                                self.read_state = BufferReadState::Closed;
                                tracing::trace!("bufprotocolreader read ok empty(EOF), will close");
                                return Poll::Ready(Ok(None));
                            }

                            self.read_header(cx, rc, 0, l)
                        }
                        Err(e) => {
                            let _ = self.read_cache.insert(rc);

                            Poll::Ready(Err(e))
                        }
                    },
                    Poll::Pending => {
                        let _ = self.read_cache.insert(rc);

                        Poll::Pending
                    }
                }
            }
            BufferReadState::ContinueReadRemote {
                content_len,
                body_start_index,
                filled_data_len,
            } => {
                let mut rc = rc.unwrap();

                debug_assert_eq!(self.read_cap, rc.len());

                let mut rb = ReadBuf::new(&mut rc);

                rb.set_filled(filled_data_len);

                match self.reader.as_mut().poll_read(cx, &mut rb) {
                    Poll::Pending => {
                        let _ = self.read_cache.insert(rc);

                        Poll::Pending
                    }
                    Poll::Ready(r) => match r {
                        Err(e) => {
                            let _ = self.read_cache.insert(rc);

                            Poll::Ready(Err(e))
                        }
                        Ok(_) => {
                            let data = rb.filled();
                            let dl = data.len();

                            debug_assert!(dl >= filled_data_len);

                            if dl == filled_data_len {
                                trace!("  ContinueReadRemote got empty(EOF), will close");
                                self.read_state = BufferReadState::Closed;
                                return Poll::Ready(Ok(None));
                            }

                            let real_len = data[body_start_index..].len();

                            if real_len < content_len {
                                tracing::trace!("partial read2: {real_len} {content_len}",);

                                self.read_state = BufferReadState::ContinueReadRemote {
                                    content_len,
                                    body_start_index,
                                    filled_data_len: dl,
                                };

                                let _ = self.read_cache.insert(rc);

                                self.read(cx)
                            } else {
                                if tracing::enabled!(tracing::Level::TRACE) {
                                    let real_data =
                                        &data[body_start_index..body_start_index + content_len];
                                    let real_string = String::from_utf8_lossy(real_data);

                                    tracing::trace!(
                                        "partial read2 finish, {}, {}, {}",
                                        real_len,
                                        content_len,
                                        real_string.len()
                                    );
                                }

                                if real_len > content_len {
                                    self.read_state = BufferReadState::ContinueReadLocalCache {
                                        from: body_start_index + content_len,
                                        to: dl,
                                    }
                                } else {
                                    self.read_state = BufferReadState::ReadyForNew;
                                }

                                Poll::Ready(Ok(Some(BufReadResult {
                                    buf: rc,
                                    body_from: body_start_index,
                                    body_to: body_start_index + content_len,
                                })))
                            }
                        }
                    },
                }
            }
        }
    }

    /// It extracts body len by using self.content_len_body_start_index_parse_fn, then
    /// if the cached `rc` is not less than the body len, it returns the result, or else
    /// it calls self.read to read until it got the full body.
    ///
    fn read_header(
        &mut self,
        cx: &mut Context<'_>,
        rc: BytesMut,
        from: usize,
        to: usize,
    ) -> Poll<std::io::Result<Option<BufReadResult>>> {
        debug_assert!(from < to);

        let data = &rc[from..to];
        let PacketMetadata {
            content_len,
            body_start_index,
        } = (self.content_len_body_start_index_parse_fn)(data)?;
        let real_len = data[body_start_index..].len();

        match content_len.cmp(&real_len) {
            std::cmp::Ordering::Less => {
                debug_assert!(from + body_start_index + content_len < to);

                self.read_state = BufferReadState::ContinueReadLocalCache {
                    from: from + body_start_index + content_len,
                    to,
                };

                Poll::Ready(Ok(Some(BufReadResult {
                    buf: rc,
                    body_from: from + body_start_index,
                    body_to: from + body_start_index + content_len,
                })))
            }
            std::cmp::Ordering::Equal => {
                self.read_state = BufferReadState::ReadyForNew;
                Poll::Ready(Ok(Some(BufReadResult {
                    buf: rc,
                    body_from: from + body_start_index,
                    body_to: from + body_start_index + content_len,
                })))
            }
            std::cmp::Ordering::Greater => {
                let mut new_rc = BytesMut::with_capacity(self.read_cap);

                new_rc.extend_from_slice(data);

                unsafe {
                    new_rc.set_len(self.read_cap);
                }

                self.read_state = BufferReadState::ContinueReadRemote {
                    content_len,
                    body_start_index,
                    filled_data_len: data.len(),
                };

                let _ = self.read_cache.insert(new_rc);

                self.read(cx)
            }
        }
    }
}
