use super::*;

use core::time;
use std::{
    io,
    ops::DerefMut,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::ReadBuf;

// 整个 文件的内容都是在模仿 AsyncRead 和 AsyncWrite 的实现,
// 只是加了一个 Addr 参数. 这一部分比较难懂。

/// 每一次读都获取到一个 Addr,
pub trait AsyncReadAddr: crate::Name {
    fn poll_read_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>>;
}

/// 每一次写都写入一个 Addr
pub trait AsyncWriteAddr: crate::Name {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>>;

    fn poll_flush_addr(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>>;

    fn poll_close_addr(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
}

pub struct AddrConn {
    pub r: Box<dyn AddrReadTrait>,
    pub w: Box<dyn AddrWriteTrait>,

    pub default_write_to: Option<Addr>,

    cached_name: String,
}
impl Name for AddrConn {
    fn name(&self) -> &str {
        &self.cached_name
    }
}
impl AddrConn {
    pub fn new(r: Box<dyn AddrReadTrait>, w: Box<dyn AddrWriteTrait>) -> Self {
        let cached_name = match r.name() == w.name() {
            true => String::from(r.name()),
            false => format!("({}_{})", r.name(), w.name()),
        };
        AddrConn {
            r,
            w,
            default_write_to: None,
            cached_name,
        }
    }
}

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
            addr: &Addr,
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

/*

////////////////////////////////////////////////////////////////////

                        shutdown part

////////////////////////////////////////////////////////////////////
*/
//tokio::io::util::shutdown

pin_project_lite::pin_project! {
    /// A future used to shutdown an I/O object.
    ///
    /// Created by the [`AsyncWriteExt::shutdown`][shutdown] function.
    /// [shutdown]: [`crate::io::AsyncWriteExt::shutdown`]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    #[derive(Debug)]
    pub struct Shutdown<'a, A: ?Sized> {
        a: &'a mut A,
        // Make this future `!Unpin` for compatibility with async trait methods.
        #[pin]
        _pin: std::marker::PhantomPinned,
    }
}

/// Creates a future which will shutdown an I/O object.
pub(super) fn shutdown<A>(a: &mut A) -> Shutdown<'_, A>
where
    A: AsyncWriteAddr + Unpin + ?Sized,
{
    Shutdown {
        a,
        _pin: std::marker::PhantomPinned,
    }
}

impl<A> futures_util::Future for Shutdown<'_, A>
where
    A: AsyncWriteAddr + Unpin + ?Sized,
{
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = self.project();
        Pin::new(me.a).poll_close_addr(cx)
    }
}

/*

////////////////////////////////////////////////////////////////////

                        end shutdown part

////////////////////////////////////////////////////////////////////
*/

pub trait AsyncWriteAddrExt: AsyncWriteAddr {
    fn write<'a>(&'a mut self, buf: &'a [u8], addr: &'a Addr) -> WriteFuture<'a, Self>
    where
        Self: Unpin,
    {
        WriteFuture {
            writer: self,
            buf,
            addr,
        }
    }

    fn shutdown(&mut self) -> Shutdown<'_, Self>
    where
        Self: Unpin,
    {
        shutdown(self)
    }
}
impl<T: AsyncWriteAddr + ?Sized> AsyncWriteAddrExt for T {}

pub struct WriteFuture<'a, T: Unpin + ?Sized> {
    pub(crate) writer: &'a mut T,
    pub(crate) buf: &'a [u8],
    pub(crate) addr: &'a Addr,
}

impl<T: AsyncWriteAddr + Unpin + ?Sized> WriteFuture<'_, T> {
    fn poll_w(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let buf = self.buf;
        Pin::new(&mut *self.writer).poll_write_addr(cx, buf, self.addr)
    }
}

impl<T: AsyncWriteAddr + Unpin + ?Sized> futures::Future for WriteFuture<'_, T> {
    type Output = io::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.poll_w(cx)
    }
}

/*

////////////////////////////////////////////////////////////////////

                        End of Traits

////////////////////////////////////////////////////////////////////
*/

pub const CP_UDP_TIMEOUT: time::Duration = Duration::from_secs(100); //todo: change this
pub const MAX_DATAGRAM_SIZE: usize = 65535 - 20 - 8;

/// 循环读写直到read错误发生. 不会认为 read错误为错误. 每一次read都会以
/// CP_UDP_TIMEOUT 为 最长等待时间，一旦读不到，就会退出函数
///
/// 读到后，如果写超过了同样的时间，也退出函数
pub async fn cp_addr<R1: AddrReadTrait, W1: AddrWriteTrait>(
    mut r1: R1,
    mut w1: W1,
) -> Result<u64, Error> {
    let mut whole_write = 0;

    loop {
        let r1ref = &mut r1;

        let sleepf = tokio::time::sleep(CP_UDP_TIMEOUT).fuse();
        let readf = async move {
            let mut buf0 = Box::new([0u8; MAX_DATAGRAM_SIZE]);
            let mut buf = ReadBuf::new(buf0.deref_mut());
            let r = r1ref.read(buf.initialized_mut()).await;

            (r, buf0)
        }
        .fuse();
        pin_mut!(sleepf, readf);

        futures::select! {
            _ = sleepf =>{
                debug!("read addrconn timeout");

                break;
            }
            r = readf =>{
                let (r,  buf0) = r;
                match r {
                    Err(_) => break,
                    Ok((m, ad)) => {
                        if m > 0 {
                            //写udp 是不会卡住的, 但addr_conn底层可能不是 udp

                            let sleepf2 = tokio::time::sleep(CP_UDP_TIMEOUT).fuse();
                            let wf = w1.write(&buf0[..m], &ad).fuse();

                            pin_mut!(sleepf2, wf);
                            futures::select!{
                                _ = sleepf2 =>{
                                     debug!("write addrconn timeout");
                                }
                                r = wf =>{
                                    if let Err(e) = r {
                                        debug!("write addrconn got err, {}",e);
                                        break;
                                    }
                                }
                            }

                        }
                        whole_write += m;
                    }
                }
            }
        } //select
    } //loop

    Ok(whole_write as u64)
}

/// copy data between two AddrConn struct
pub async fn cp(
    cid: CID,
    c1: AddrConn,
    c2: AddrConn,
    opt: Option<Arc<GlobalTrafficRecorder>>,
) -> Result<u64, Error> {
    cp_between(cid, c1.r, c1.w, c2.r, c2.w, opt).await
}

pub async fn cp_between<
    R1: AddrReadTrait,
    R2: AddrReadTrait,
    W1: AddrWriteTrait,
    W2: AddrWriteTrait,
>(
    cid: CID,
    r1: R1,
    w1: W1,
    r2: R2,
    w2: W2,
    opt: Option<Arc<GlobalTrafficRecorder>>,
) -> Result<u64, Error> {
    let (c1_to_c2, c2_to_c1) = (cp_addr(r1, w2).fuse(), cp_addr(r2, w1).fuse());
    pin_mut!(c1_to_c2, c2_to_c1);

    futures::select! {
        rst1 = c1_to_c2 =>{

            if tracing::enabled!(tracing::Level::DEBUG)  {
                debug!( cid = %cid,"cp_addr end, u");
            }

            if let Some(gtr) = opt.as_ref(){
                match &rst1{
                    Ok(n) => {
                        let tt = gtr.ub.fetch_add(*n, Ordering::Relaxed);
                        debug!(cid = %cid,"cp_addr_end, u, ub, {},{}",n,tt+n);
                    },
                    Err(e) => {
                        debug!(cid = %cid,"cp_addr_end with err, u, {}",e);

                    },
                }

            }


            let rst2 = c2_to_c1.await;

            if tracing::enabled!(tracing::Level::DEBUG)  {
                debug!(cid = %cid,"cp_addr end, d");
            }

            if let Some(gtr) = opt{
                match &rst2{
                    Ok(n) => {
                        let tt = gtr.db.fetch_add(*n, Ordering::Relaxed);
                        debug!(cid = %cid,"cp_addr_end, u ,db, {},{}",n,tt+n);
                    },
                    Err(e) => {
                        debug!(cid = %cid,"cp_addr_end with err, u, d, {}",e);

                    },
                }
            }

            rst1
        }
        rst2 = c2_to_c1 =>{
            if tracing::enabled!(tracing::Level::DEBUG)  {
                debug!(cid = %cid,"cp_addr end, d");
            }
            if let Some(gtr) = opt.as_ref(){
                match &rst2{
                    Ok(n) => {
                        let tt = gtr.db.fetch_add(*n, Ordering::Relaxed);
                        debug!(cid = %cid,"cp_addr_end, d, db, {},{}",n,tt+n);
                    },
                    Err(e) => {
                        debug!(cid = %cid,"cp_addr_end with err, d, d, {}",e);

                    },
                }

            }

            let rst1 = c1_to_c2.await;
            if tracing::enabled!(tracing::Level::DEBUG)  {
                debug!(cid = %cid,"cp_addr end, u, ");
            }
            if let Some(gtr) = opt{

                match &rst1{
                    Ok(n) => {
                        let tt = gtr.ub.fetch_add(*n, Ordering::Relaxed);
                        debug!(cid = %cid,"cp_addr_end, d, ub, {},{}",n,tt+n);
                    },
                    Err(e) => {
                        debug!(cid = %cid,"cp_addr_end with err, d, u, {}",e);

                    },
                }
            }
            rst2
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    struct MyType {
        counter: u32,
    }
    impl crate::Name for MyType {
        fn name(&self) -> &str {
            "mytype"
        }
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
            _addr: &Addr,
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
        obj2.write(buf, &a).await
    }
}
