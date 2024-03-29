/*!
Defines Maps for h2 and grpc

目前的设定: grpc client 默认使用 0rtt; 非 grpc 的 h2 不使用0rtt
*/

pub mod client;
pub mod grpc;
pub mod server;

use std::cmp::min;
use std::io::{Error, ErrorKind, Result};
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Bytes, BytesMut};
use futures::ready;

use h2::{RecvStream, SendStream};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::oneshot::Sender;
use tracing::debug;

// (MIT)
// https://github.com/zephyrchien/midori/blob/master/src/transport/h2/stream.rs

const BUFFER_CAP: usize = 0x4000; //todo: adjust this

pub struct H2Stream {
    recv: RecvStream,
    send: SendStream<Bytes>,
    buffer: BytesMut,
    shutdown_tx: Option<Sender<()>>,
}

impl H2Stream {
    #[inline]
    pub fn new(recv: RecvStream, send: SendStream<Bytes>, shutdown_tx: Option<Sender<()>>) -> Self {
        H2Stream {
            recv,
            send,
            buffer: BytesMut::with_capacity(BUFFER_CAP),
            shutdown_tx,
        }
    }
}

impl Drop for H2Stream {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            debug!("h2 got dropped, sending shutdown_tx ");
            let _ = tx.send(());
        }
    }
}

impl AsyncRead for H2Stream {
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        if !self.buffer.is_empty() {
            let r_len = min(buf.remaining(), self.buffer.len());
            let data = self.buffer.split_to(r_len);
            buf.put_slice(&data[..r_len]);
            return Poll::Ready(Ok(()));
        };
        Poll::Ready(match ready!(self.recv.poll_data(cx)) {
            Some(Ok(data)) => {
                let r_len = min(buf.remaining(), data.len());
                buf.put_slice(&data[..r_len]);
                // copy the left payload into buffer
                if data.len() > r_len {
                    self.buffer.extend_from_slice(&data[r_len..]);
                };
                // increase recv window
                self.recv
                    .flow_control()
                    .release_capacity(r_len)
                    .map_err(|e| Error::new(ErrorKind::ConnectionReset, e))
            }
            // no more data frames
            // maybe trailer
            // or cancelled
            _ => Ok(()),
        })
    }
}

impl AsyncWrite for H2Stream {
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize>> {
        self.send.reserve_capacity(buf.len());
        Poll::Ready(match ready!(self.send.poll_capacity(cx)) {
            Some(Ok(to_write)) => self
                .send
                .send_data(Bytes::from(buf[..to_write].to_owned()), false)
                .map_or_else(
                    |e| Err(Error::new(ErrorKind::BrokenPipe, e)),
                    |_| Ok(to_write),
                ),
            // is_send_streaming returns false
            // which indicates the state is
            // neither open nor half_close_remote
            _ => Err(Error::new(ErrorKind::BrokenPipe, "broken pipe")),
        })
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        self.send.reserve_capacity(0);
        Poll::Ready(ready!(self.send.poll_capacity(cx)).map_or(
            Err(Error::new(ErrorKind::BrokenPipe, "broken pipe")),
            |_| {
                let r = self
                    .send
                    .send_data(Bytes::new(), true)
                    .map_err(|e| Error::new(ErrorKind::BrokenPipe, e));

                if let Some(tx) = self.shutdown_tx.take() {
                    debug!("h2 sending shutdown_tx ");
                    let _ = tx.send(());
                }
                r
            },
        ))
    }
}
