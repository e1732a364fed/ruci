/*!
Defines Mappers for quic protocol. uses quinn.

 */

pub mod client;
pub mod server;

use std::io::Result;
use std::pin::Pin;
use std::task::{Context, Poll};

use quinn::{RecvStream, SendStream};
use tokio::io::{AsyncRead, AsyncWrite};

pub struct Stream {
    send: SendStream,
    recv: RecvStream,
}

impl Stream {
    #[inline]
    pub fn new(send: SendStream, recv: RecvStream) -> Self {
        Stream { send, recv }
    }
}

impl AsyncRead for Stream {
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for Stream {
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize>> {
        Pin::new(&mut self.send).poll_write(cx, buf)
    }

    #[inline]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        Pin::new(&mut self.send).poll_flush(cx)
    }

    #[inline]
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        Pin::new(&mut self.send).poll_shutdown(cx)
    }
}
