pub mod client;
pub mod server;

use std::{io, pin::Pin, task::Poll};

use bytes::{Buf, Bytes, BytesMut};
use futures::Sink;
use futures_lite::{ready, StreamExt};
use ruci::net::ConnTrait;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use ruci::utils::{io_error, io_error2};

pub const MAX_EARLY_DATA_LEN_BASE64: usize = 2732;
pub const MAX_EARLY_DATA_LEN: usize = 2048;
pub const EARLY_DATA_HEADER_KEY: &str = "Sec-WebSocket-Protocol";

pub struct WsStreamToConnWrapper<T: ConnTrait> {
    ws: Pin<Box<WebSocketStream<T>>>,
    r_buf: Option<Bytes>,
    w_buf: Option<BytesMut>,
}

impl<T: ConnTrait> ruci::Name for WsStreamToConnWrapper<T> {
    fn name(&self) -> &str {
        "websocket_conn"
    }
}

impl<T: ConnTrait> AsyncRead for WsStreamToConnWrapper<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut core::task::Context,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            if let Some(read_buf) = &mut self.r_buf {
                if read_buf.len() <= buf.remaining() {
                    buf.put_slice(read_buf);
                    self.r_buf = None;
                } else {
                    let len = buf.remaining();
                    buf.put_slice(&read_buf[..len]);
                    read_buf.advance(len);
                }
                return Poll::Ready(Ok(()));
            }
            let message = ready!(self.ws.as_mut().poll_next(cx));
            if message.is_none() {
                return Poll::Ready(Err(io_error("ws stream got none message")));
            }
            let message = message.unwrap().map_err(io_error)?;
            match message {
                Message::Binary(binary) => {
                    if binary.len() < buf.remaining() {
                        buf.put_slice(&binary);
                        return Poll::Ready(Ok(()));
                    } else {
                        self.r_buf = Some(Bytes::from(binary));
                        continue;
                    }
                }
                Message::Close(_) => {
                    return Poll::Ready(Ok(()));
                }
                _ => {
                    return Poll::Ready(Err(io_error2(
                        "ws stream got message type other than binary or close ",
                        message,
                    )))
                }
            }
        }
    }
}

impl<T: ConnTrait> AsyncWrite for WsStreamToConnWrapper<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut core::task::Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        ready!(self.ws.as_mut().poll_ready(cx)).map_err(io_error)?;

        let message = if let Some(ref mut b) = self.w_buf.take() {
            b.extend_from_slice(buf);

            Message::Binary((&**b).into())
        } else {
            Message::Binary(buf.into())
        };

        self.ws.as_mut().start_send(message).map_err(io_error)?;
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut core::task::Context,
    ) -> Poll<Result<(), io::Error>> {
        self.ws.as_mut().poll_flush(cx).map_err(io_error)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut core::task::Context,
    ) -> Poll<Result<(), io::Error>> {
        ready!(self.ws.as_mut().poll_ready(cx)).map_err(io_error)?;
        let message = Message::Close(None);
        let _ = self.ws.as_mut().start_send(message);

        let inner = self.ws.as_mut();
        inner
            .poll_close(cx)
            .map_err(|e| io_error2("ws close got err:", e))
    }
}
