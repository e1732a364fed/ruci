/*!
这里说的 0rtt 是指在 h2 握手后, 客户端发送 "建立子连接请求" 后, 无须等待服务端
的回复确认包 而直接发送 子连接初始数据的 做法.

也可以叫 early data
 */

use futures_lite::FutureExt;
use h2::client::ResponseFuture;

use crate::map::h2::BUFFER_CAP;

use super::*;

/// 0rtt Stream, store a ResponseFuture at first, poll it until it returns a RecvStream
pub struct Stream {
    recv: Option<RecvStream>,
    send: SendStream<Bytes>,
    resp_f: Option<Pin<Box<ResponseFuture>>>,
    r_buffer: BytesMut,

    next_data_len: usize,

    shutdown_tx: Option<Sender<()>>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            debug!("grpc got dropped, sending shutdown_tx ");
            let _ = tx.send(());
        }
    }
}

impl Stream {
    #[inline]
    pub fn new(
        rf: ResponseFuture,
        send: SendStream<Bytes>,
        shutdown_tx: Option<Sender<()>>,
    ) -> Self {
        //debug!("new grpc stream");
        Stream {
            recv: None,
            resp_f: Some(Box::pin(rf)),
            send,
            r_buffer: BytesMut::with_capacity(BUFFER_CAP),
            next_data_len: 0,
            shutdown_tx,
        }
    }

    fn try_read_next_len(&mut self) -> Result<(), UVariantErr> {
        if self.next_data_len == 0 {
            self.r_buffer.advance(READ_HEAD_LEN);
            let (len, oe) = read_uvarint(&mut self.r_buffer);
            if let Some(e) = oe {
                return Err(e);
            }
            self.next_data_len = len as usize;
        }
        Ok(())
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            if !self.r_buffer.is_empty() && self.r_buffer.len() >= self.next_data_len {
                let r = self.try_read_next_len();
                if let Err(e) = r {
                    return Poll::Ready(Err(io_error(e)));
                }

                let v = vec![buf.remaining(), self.r_buffer.len(), self.next_data_len];

                let r_len = v.into_iter().min().expect("ok");

                let read_data = self.r_buffer.split_to(r_len);
                buf.put_slice(&read_data);

                self.next_data_len -= r_len;

                return Poll::Ready(Ok(()));
            };

            let r_data = {
                let rv = if let Some(rv) = self.recv.as_mut() {
                    rv
                } else {
                    let r = self.resp_f.as_mut().unwrap().poll(cx);
                    match r {
                        Poll::Ready(r) => match r {
                            Ok(r) => {
                                self.recv = Some(r.into_body());
                                self.resp_f = None;
                                self.recv.as_mut().unwrap()
                            }
                            Err(e) => return Poll::Ready(Err(io_error(e.to_string()))),
                        },
                        Poll::Pending => return Poll::Pending,
                    }
                };
                ready!(rv.poll_data(cx))
            };

            match r_data {
                Some(Ok(data)) => {
                    self.r_buffer.extend_from_slice(&data);

                    let r = self
                        .recv
                        .as_mut()
                        .unwrap()
                        .flow_control()
                        .release_capacity(data.len())
                        .map_err(|e| Error::new(ErrorKind::ConnectionReset, e));
                    if let Err(e) = r {
                        return Poll::Ready(Err(e));
                    }
                }
                _ => return Poll::Ready(Ok(())),
            }
        }
    }
}

impl AsyncWrite for Stream {
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let old_len = buf.len();
        let realbuf_len = get_real_len(old_len);

        self.send.reserve_capacity(realbuf_len);
        Poll::Ready(match ready!(self.send.poll_capacity(cx)) {
            Some(Ok(write_len)) => {
                if write_len >= realbuf_len {
                    let b = encode(buf, false);

                    self.send.send_data(b.into(), false).map_or_else(
                        |e| Err(Error::new(ErrorKind::BrokenPipe, e)),
                        |_| Ok(min(old_len, write_len)),
                    )
                } else {
                    let diff = realbuf_len - write_len;
                    let old_buf_written = old_len - diff;

                    let to_write_buf = encode(&buf[..old_buf_written], false);

                    self.send.send_data(to_write_buf.into(), false).map_or_else(
                        |e| Err(Error::new(ErrorKind::BrokenPipe, e)),
                        |_| Ok(old_buf_written),
                    )
                }
            }

            _ => Err(Error::new(ErrorKind::BrokenPipe, "broken pipe")),
        })
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.send.reserve_capacity(0);
        Poll::Ready(ready!(self.send.poll_capacity(cx)).map_or(
            Err(Error::new(ErrorKind::BrokenPipe, "broken pipe")),
            |_| {
                let r = self
                    .send
                    .send_data(Bytes::new(), true)
                    .map_err(|e| Error::new(ErrorKind::BrokenPipe, e));

                if let Some(tx) = self.shutdown_tx.take() {
                    debug!("grpc sending shutdown_tx ");
                    let _ = tx.send(());
                }
                r
            },
        ))
    }
}
