/*!
Defines a channel based [`TcpStream`] created by [`super::SmoltcpDevice`]
*/
use std::{
    io::Error,
    net::SocketAddr,
    pin::Pin,
    task::{ready, Context, Poll},
};

use bytes::{Buf, BytesMut};
use smoltcp::iface::SocketHandle;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::mpsc::{Receiver, Sender},
};
use tokio_util::sync::PollSender;

pub struct TcpReadHalf {
    rx: Receiver<BytesMut>,
    peer_addr: SocketAddr,
    buf: BytesMut,
}

impl TcpReadHalf {
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }
    pub fn close(&mut self) {
        self.rx.close();
    }
}

pub struct TcpWriteHalf {
    h: SocketHandle,
    tx: PollSender<(SocketHandle, SocketAddr, BytesMut)>,
    local_addr: SocketAddr,
}

pub struct TcpStream {
    r: TcpReadHalf,
    w: TcpWriteHalf,
    local_addr: SocketAddr,
    peer_addr: SocketAddr,
}

impl TcpStream {
    pub(crate) fn new(
        rx: Receiver<BytesMut>,
        tx: Sender<(SocketHandle, SocketAddr, BytesMut)>,
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
        h: SocketHandle,
    ) -> Self {
        Self {
            r: TcpReadHalf {
                rx,
                peer_addr,
                buf: BytesMut::new(),
            },
            w: TcpWriteHalf {
                h,
                tx: PollSender::new(tx),
                local_addr,
            },
            local_addr,
            peer_addr,
        }
    }

    pub fn into_split(self) -> (TcpReadHalf, TcpWriteHalf) {
        (self.r, self.w)
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }
}

impl AsyncRead for TcpReadHalf {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let me = self.get_mut();
        if !me.buf.is_empty() {
            let dst_buffer = buf.initialize_unfilled();
            let len = dst_buffer.len().min(me.buf.len());
            let _ = &dst_buffer[..len].copy_from_slice(&me.buf.as_ref()[..len]);
            me.buf.advance(len);
            buf.set_filled(buf.filled().len() + len);
            Poll::Ready(Ok(()))
        } else if let Some(data) = ready!(me.rx.poll_recv(cx)) {
            me.buf = data;
            Pin::new(me).poll_read(cx, buf)
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

impl AsyncWrite for TcpWriteHalf {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        let me = self.get_mut();
        if ready!(me.tx.poll_reserve(cx)).is_ok() {
            if let Err(err) = me.tx.send_item((me.h, me.local_addr, buf.into())) {
                tracing::warn!("tcp send response failed: {}", err);
            } else {
                return Poll::Ready(Ok(buf.len()));
            }
        }
        Poll::Ready(Ok(0))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        self.poll_write(cx, &[]).map(|ret| ret.map(|_| ()))
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let me = self.get_mut();
        Pin::new(&mut me.r).poll_read(cx, buf)
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        let me = self.get_mut();
        Pin::new(&mut me.w).poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let me = self.get_mut();
        Pin::new(&mut me.w).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let me = self.get_mut();
        me.r.rx.close();
        Pin::new(&mut me.w).poll_shutdown(cx)
    }
}
