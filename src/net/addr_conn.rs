use super::*;

use bytes::{Buf, BytesMut};
use futures::io::Error;
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

// 整个 文件的内容都是在模仿 AsyncRead 和 AsyncWrite 的实现,
// 只是加了一个 Addr 参数. 这一部分比较难懂。

/// 每一次读都获取到一个 Addr,
pub trait AsyncReadAddr {
    fn poll_read_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>>;
}

/// 每一次写都写入一个 Addr
pub trait AsyncWriteAddr {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: Addr,
    ) -> Poll<io::Result<usize>>;

    fn poll_flush_addr(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>>;

    fn poll_close_addr(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
}

pub struct AddrConn(pub Box<dyn AddrReadTrait>, pub Box<dyn AddrWriteTrait>);

/*

////////////////////////////////////////////////////////////////////

                        Read part

////////////////////////////////////////////////////////////////////
*/

pub trait AddrReadTrait: AsyncReadAddr + Unpin + Send + Sync {}
impl<T: AsyncReadAddr + Unpin + Send + Sync> AddrReadTrait for T {}

macro_rules! deref_async_read_addr {
    () => {
        fn poll_read_addr(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<io::Result<(usize, Addr)>> {
            Pin::new(&mut **self).poll_read_addr(cx, buf)
        }
    };
}

impl<T: ?Sized + AsyncReadAddr + Unpin> AsyncReadAddr for Box<T> {
    deref_async_read_addr!();
}

impl<T: ?Sized + AsyncReadAddr + Unpin> AsyncReadAddr for &mut T {
    deref_async_read_addr!();
}

pub trait AsyncReadAddrExt: AsyncReadAddr {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> ReadAddrFuture<'a, Self>
    where
        Self: Unpin,
    {
        ReadAddrFuture { reader: self, buf }
    }
}
impl<T: AsyncReadAddr + ?Sized> AsyncReadAddrExt for T {}

pub struct ReadAddrFuture<'a, T: Unpin + ?Sized> {
    pub(crate) reader: &'a mut T,
    pub(crate) buf: &'a mut [u8],
}

impl<T: AsyncReadAddr + Unpin + ?Sized> futures::Future for ReadAddrFuture<'_, T> {
    type Output = io::Result<(usize, Addr)>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self { reader, buf } = &mut *self;
        Pin::new(reader).poll_read_addr(cx, buf)
    }
}

/*

////////////////////////////////////////////////////////////////////

                        Write part

////////////////////////////////////////////////////////////////////
*/

pub trait AddrWriteTrait: AsyncWriteAddr + Unpin + Send + Sync {}
impl<T: AsyncWriteAddr + Unpin + Send + Sync> AddrWriteTrait for T {}

macro_rules! deref_async_write_addr {
    () => {
        fn poll_write_addr(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
            addr: Addr,
        ) -> Poll<io::Result<usize>> {
            Pin::new(&mut **self).poll_write_addr(cx, buf, addr)
        }

        fn poll_flush_addr(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut **self).poll_flush_addr(cx)
        }

        fn poll_close_addr(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut **self).poll_close_addr(cx)
        }
    };
}

impl<T: ?Sized + AsyncWriteAddr + Unpin> AsyncWriteAddr for Box<T> {
    deref_async_write_addr!();
}

impl<T: ?Sized + AsyncWriteAddr + Unpin> AsyncWriteAddr for &mut T {
    deref_async_write_addr!();
}

pub trait AsyncWriteAddrExt: AsyncWriteAddr {
    fn write<'a>(&'a mut self, buf: &'a [u8], addr: Addr) -> WriteFuture<'a, Self>
    where
        Self: Unpin,
    {
        WriteFuture {
            writer: self,
            buf,
            addr,
        }
    }
}
impl<T: AsyncWriteAddr + ?Sized> AsyncWriteAddrExt for T {}

pub struct WriteFuture<'a, T: Unpin + ?Sized> {
    pub(crate) writer: &'a mut T,
    pub(crate) buf: &'a [u8],
    pub(crate) addr: Addr,
}

impl<T: AsyncWriteAddr + Unpin + ?Sized> futures::Future for WriteFuture<'_, T> {
    type Output = io::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let buf = self.buf;
        let addr = self.addr.clone();
        Pin::new(&mut *self.writer).poll_write_addr(cx, buf, addr)
    }
}

/*

////////////////////////////////////////////////////////////////////

                        End of Traits

////////////////////////////////////////////////////////////////////
*/

/// 循环读写直到read错误发生. 不会认为 read错误为错误
pub async fn cp_addr<R1: AddrReadTrait, W1: AddrWriteTrait>(
    mut r1: R1,
    mut w1: W1,
) -> Result<u64, Error> {
    const CAP: usize = 1500;
    let mut buf: BytesMut = BytesMut::with_capacity(CAP);
    let mut whole_write = 0;
    loop {
        buf.resize(CAP, 0);
        let r = r1.read(&mut buf).await;
        if r.is_err() {
            break;
        }
        let (m, ad) = r.unwrap();
        if m > 0 {
            buf.truncate(m);
            loop {
                let r = w1.write(&mut buf, ad.clone()).await;
                if r.is_err() {
                    break;
                }
                let n = r.unwrap();
                buf.advance(n);
                if buf.len() == 0 {
                    break;
                }
            }
        }
        whole_write += m;
    }

    Ok(whole_write as u64)
}

pub async fn cp_addrconn<F: Fn() -> ()>(
    cid: u32,
    c1: AddrConn,
    c2: AddrConn,
    shutdown_f: F,
    opt: Option<Arc<TransmissionInfo>>,
) -> Result<u64, Error> {
    cp_addr_between(cid, c1.0, c1.1, c2.0, c2.1, shutdown_f, opt).await
}

pub async fn cp_addr_between<
    F: Fn() -> (),
    R1: AddrReadTrait,
    R2: AddrReadTrait,
    W1: AddrWriteTrait,
    W2: AddrWriteTrait,
>(
    cid: u32,
    r1: R1,
    w1: W1,
    r2: R2,
    w2: W2,
    shutdown_f: F,
    _opt: Option<Arc<TransmissionInfo>>,
) -> Result<u64, Error> {
    let (c1_to_c2, c2_to_c1) = (cp_addr(r1, w2).fuse(), cp_addr(r2, w1).fuse());
    pin_mut!(c1_to_c2, c2_to_c1);

    select! {
        r1 = c1_to_c2 =>{
            debug!("cid: {}, cp_addr_end, r1",cid);
            shutdown_f();
            r1
        }
        r2 = c2_to_c1 =>{
            debug!("cid: {}, cp_addr_end, r2",cid);

            shutdown_f();
            r2
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    struct MyType {
        counter: u32,
    }

    impl AsyncReadAddr for MyType {
        fn poll_read_addr(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _buf: &mut [u8],
        ) -> Poll<io::Result<(usize, Addr)>> {
            self.counter += 1;
            if self.counter <= 5 {
                Poll::Pending
            } else {
                Poll::Ready(Ok((0, Addr::default())))
            }
        }
    }

    impl AsyncWriteAddr for MyType {
        fn poll_write_addr(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &[u8],
            _addr: Addr,
        ) -> Poll<io::Result<usize>> {
            if self.counter <= 5 {
                Poll::Pending
            } else {
                Poll::Ready(Ok(0))
            }
        }

        fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    // 在 async 函数中调用异步的 trait 方法
    async fn _my_async_function(
        mut obj: Box<dyn AddrReadTrait>,
        buf: &mut [u8],
    ) -> io::Result<(usize, Addr)> {
        obj.read(buf).await
    }

    async fn _my_async_function2(
        mut obj2: Box<dyn AddrWriteTrait>,
        buf: &mut [u8],
    ) -> io::Result<usize> {
        let a = Addr::default();
        obj2.write(buf, a).await
    }
}
