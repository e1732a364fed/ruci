use std::{io, pin::Pin, task::Poll};

use super::*;
use bytes::BufMut;
use parking_lot::Mutex;
use tokio::io::ReadBuf;

use futures::task::Context;

use std::cmp::min;

/// useful for testing
#[derive(Debug)]
pub struct MockTcpStream {
    pub read_data: Vec<u8>,
    pub write_data: Vec<u8>,
    pub write_target: Option<Arc<Mutex<Vec<u8>>>>,
}
impl crate::Name for MockTcpStream {
    fn name(&self) -> &str {
        "mock_tcp_stream"
    }
}

impl Unpin for MockTcpStream {}
impl AsyncRead for MockTcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        //debug!("MockTcp: read called");
        let size: usize = min(self.read_data.len(), buf.remaining());
        if size == 0 {
            return Poll::Ready(Ok(()));
        }
        buf.put(&self.read_data[..size]);

        let new_len = self.read_data.len() - size;

        self.read_data.copy_within(size.., 0);
        self.read_data.truncate(new_len);

        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for MockTcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        let mut bv = Vec::from(buf);

        if let Some(swt) = &self.write_target {
            let mut v = swt.lock();
            v.append(&mut bv);
        } else if self.write_data.is_empty() {
            self.write_data = bv;
        } else {
            self.write_data.append(&mut bv)
        }

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }
}

/// useful for testing
#[derive(Debug)]
pub struct MockTcpStream2<'a> {
    pub read_data: &'a mut Vec<u8>,
    pub write_data: &'a mut Vec<u8>,
    pub write_target: Option<Arc<Mutex<Vec<u8>>>>,
}
impl<'a> crate::Name for MockTcpStream2<'a> {
    fn name(&self) -> &str {
        "mock_tcp_stream2"
    }
}

impl<'a> Unpin for MockTcpStream2<'a> {}
impl<'a> AsyncRead for MockTcpStream2<'a> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        //debug!("MockTcp: read called");
        let size: usize = min(self.read_data.len(), buf.remaining());
        if size == 0 {
            return Poll::Ready(Ok(()));
        }

        buf.put_slice(&self.read_data[..size]);

        let new_len = self.read_data.len() - size;

        self.read_data.copy_within(size.., 0);
        self.read_data.truncate(new_len);

        Poll::Ready(Ok(()))
    }
}

impl<'a> AsyncWrite for MockTcpStream2<'a> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        if let Some(swt) = &self.write_target {
            let mut v = swt.lock();
            v.extend_from_slice(buf);
        } else {
            self.write_data.extend_from_slice(buf);
        }

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }
}
