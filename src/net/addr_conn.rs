use crate::{utils::io_error, Name};

use super::*;

use core::time;
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::{broadcast, oneshot};

// 整个 文件的内容都是在模仿 AsyncRead 和 AsyncWrite 的实现,
// 只是加了一个 Addr 参数. 这一部分比较难懂.

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

    pub cached_name: String,
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

    fn flush(&mut self) -> Flush<'_, Self>
    where
        Self: Unpin,
    {
        flush(self)
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

pin_project_lite::pin_project! {
    /// A future used to fully flush an I/O object.
    ///
    /// Created by the [`AsyncWriteExt::flush`][flush] function.
    ///
    /// [flush]: crate::io::AsyncWriteExt::flush
    #[derive(Debug)]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    pub struct Flush<'a, A: ?Sized> {
        a: &'a mut A,
        // Make this future `!Unpin` for compatibility with async trait methods.
        #[pin]
        _pin: std::marker::PhantomPinned,
    }
}

/// Creates a future which will entirely flush an I/O object.
pub fn flush<A>(a: &mut A) -> Flush<'_, A>
where
    A: AsyncWriteAddr + Unpin + ?Sized,
{
    Flush {
        a,
        _pin: std::marker::PhantomPinned,
    }
}

impl<A> futures::Future for Flush<'_, A>
where
    A: AsyncWriteAddr + Unpin + ?Sized,
{
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = self.project();
        Pin::new(&mut *me.a).poll_flush_addr(cx)
    }
}

/*

////////////////////////////////////////////////////////////////////

                        End of Traits

////////////////////////////////////////////////////////////////////
*/

pub const CP_UDP_TIMEOUT: time::Duration = Duration::from_secs(100); //todo: change this
pub const MAX_DATAGRAM_SIZE: usize = 65535 - 20 - 8;

// todo improve the codes below

enum ReadOnceState {
    ToRead,
    ToWrite,
    ToFlush,
}

pub struct ReadOnceFuture<'a, R1: AddrReadTrait, W1: AddrWriteTrait> {
    r1: &'a mut R1,
    w1: &'a mut W1,
    buf: &'a mut [u8],

    m: usize,
    n: usize,
    ad: Addr,
    state: ReadOnceState,
}

impl<'a, R1: AddrReadTrait, W1: AddrWriteTrait> ReadOnceFuture<'a, R1, W1> {
    fn new(r1: &'a mut R1, w1: &'a mut W1, buf: &'a mut [u8]) -> Self {
        Self {
            r1,
            w1,
            buf,
            m: 0,
            n: 0,
            ad: Addr::default(),
            state: ReadOnceState::ToRead,
        }
    }
    fn poll_r(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        loop {
            match self.state {
                ReadOnceState::ToRead => {
                    let r = Pin::new(&mut *self.r1).poll_read_addr(cx, self.buf);
                    match r {
                        Poll::Pending => return Poll::Pending,

                        Poll::Ready(r) => match r {
                            Ok((m, ad)) => {
                                if m > 0 {
                                    self.m = m;
                                    self.ad = ad;

                                    self.state = ReadOnceState::ToWrite;
                                    continue;
                                } else {
                                    return Poll::Ready(Err(io_error(format!("read <=0 {m}"))));
                                }
                            }
                            Err(e) => {
                                return Poll::Ready(Err(io_error(format!(
                                    "read_once_notimeout GOT ERROR {e}"
                                ))))
                            }
                        },
                    }
                }
                ReadOnceState::ToWrite => {
                    let m = self.m;
                    let r = Pin::new(&mut *self.w1).poll_write_addr(cx, &self.buf[..m], &self.ad);
                    match r {
                        Poll::Pending => return Poll::Pending,

                        Poll::Ready(r) => match r {
                            Ok(n) => {
                                if n < m {
                                    debug!("read_once n<m {n} {m}")
                                }
                                self.n = n;
                                self.state = ReadOnceState::ToFlush;
                            }
                            Err(e) => {
                                return Poll::Ready(Err(io_error(format!(
                                    "ReadOnceFuture write GOT ERROR {e}"
                                ))))
                            }
                        },
                    }
                }
                ReadOnceState::ToFlush => {
                    let r = Pin::new(&mut *self.w1).poll_flush_addr(cx);
                    match r {
                        Poll::Ready(r) => match r {
                            Ok(_) => {
                                self.state = ReadOnceState::ToRead;
                                return Poll::Ready(Ok(self.n));
                            }
                            Err(e) => return Poll::Ready(Err(e)),
                        },
                        Poll::Pending => return Poll::Pending,
                    }
                }
            } // match state
        } // loop
    }
}

impl<'a, R1: AddrReadTrait, W1: AddrWriteTrait> futures::Future for ReadOnceFuture<'a, R1, W1> {
    type Output = io::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.poll_r(cx)
    }
}

/// 循环读写直到read错误发生. 不会认为 read错误为错误. 每一次read都会以
/// CP_UDP_TIMEOUT 为 最长等待时间, 一旦读不到, 就会退出函数
///
/// 读到后, 如果写超过了同样的时间, 也退出函数
pub async fn cp_addr<R1: AddrReadTrait + 'static, W1: AddrWriteTrait + 'static>(
    mut r1: R1,
    mut w1: W1,
    name: String,
    no_timeout: bool,
    mut shutdown_rx: broadcast::Receiver<()>,
    is_d: bool,
    opt: Option<Arc<GlobalTrafficRecorder>>,
) -> Result<u64, Error> {
    let mut whole_write = 0;
    let mut buf = Box::new([0u8; 1400]);

    loop {
        let rf = ReadOnceFuture::new(&mut r1, &mut w1, buf.as_mut());

        tokio::select! {
            r = rf =>{
                match r {
                    Ok(n) => whole_write+=n,
                    Err(e) => {
                        debug!("cp_addr got e {e}");
                        break},
                }
            }

            _ = shutdown_rx.recv() =>{
                break;
            }
        }
        tokio::task::yield_now().await;
    } //loop

    // 实测 用一个loop + 小 buf 的实现 比用 两个 spawn + mpsc 快很多. 后者非常卡顿几乎不可用
    // buf size 的选择也很重要, 太大太小都卡

    let l = whole_write as u64;
    if let Some(a) = opt {
        if is_d {
            a.db.fetch_add(l, Ordering::Relaxed);
        } else {
            a.ub.fetch_add(l, Ordering::Relaxed);
        }
    }

    Ok(l)
}

/// copy data between two AddrConn struct
#[inline]
pub async fn cp(
    cid: CID,
    c1: AddrConn,
    c2: AddrConn,
    opt: Option<Arc<GlobalTrafficRecorder>>,
    no_timeout: bool,
    shutdown_rx1: Option<tokio::sync::oneshot::Receiver<()>>,
    shutdown_rx2: Option<tokio::sync::oneshot::Receiver<()>>,
) -> Result<u64, Error> {
    cp_between(
        cid,
        c1.cached_name.clone(),
        c2.cached_name.clone(),
        c1.r,
        c1.w,
        c2.r,
        c2.w,
        opt,
        no_timeout,
        shutdown_rx1,
        shutdown_rx2,
    )
    .await
}

#[inline]
pub async fn cp_between<
    R1: AddrReadTrait + 'static,
    R2: AddrReadTrait + 'static,
    W1: AddrWriteTrait + 'static,
    W2: AddrWriteTrait + 'static,
>(
    cid: CID,
    name1: String,
    name2: String,
    r1: R1,
    w1: W1,

    r2: R2,
    w2: W2,
    opt: Option<Arc<GlobalTrafficRecorder>>,
    no_timeout: bool,
    shutdown_rx1: Option<tokio::sync::oneshot::Receiver<()>>,
    shutdown_rx2: Option<tokio::sync::oneshot::Receiver<()>>,
) -> Result<u64, Error> {
    let (tx1, rx1) = broadcast::channel(10);
    let (tx2, rx2) = broadcast::channel(10);

    let tx1c = tx1.clone();
    let tx2c = tx1.clone();

    let (_tmpx0, tmp_rx0) = oneshot::channel();
    let shutdown_rx1 = if shutdown_rx1.is_some() {
        shutdown_rx1.unwrap()
    } else {
        tmp_rx0
    };

    let (_tmpx, tmp_rx) = oneshot::channel();
    let shutdown_rx2 = if shutdown_rx2.is_some() {
        shutdown_rx2.unwrap()
    } else {
        tmp_rx
    };

    let n1 = name1.clone() + " to " + &name2;
    let n2 = name2 + " to " + &name1;

    let o1 = opt.clone();
    let o2 = opt.clone();
    let cp1 = tokio::spawn(cp_addr(r1, w2, n1, no_timeout, rx1, false, o1));
    let cp2 = tokio::spawn(cp_addr(r2, w1, n2, no_timeout, rx2, true, o2));

    let r = tokio::select! {
        r = cp1 =>{
            if tracing::enabled!(tracing::Level::DEBUG)  {
                debug!( cid = %cid,"cp_addr end, u");
            }
            let _ = tx1c.send(());
            let _ = tx2.send(());

            let r = match r{
                Ok(r) => match r {
                    Ok(d) => d,
                    Err(_) => 0,
                },
                Err(_) => 0,
            };

            r
        }
        r = cp2 =>{
            if tracing::enabled!(tracing::Level::DEBUG)  {
                debug!( cid = %cid,"cp_addr end, d");
            }
            let _ = tx1c.send(());
            let _ = tx2c.send(());

            let r = match r{
                Ok(r) => match r {
                    Ok(d) => d,
                    Err(_) => 0,
                },
                Err(_) => 0,
            };

            r
        }
         _ = shutdown_rx1 =>{
            debug!("addrconn cp_between got shutdown1 signal");

            let _ = tx1c.send(());
            let _ = tx2c.send(());

            0
        }

        _ = shutdown_rx2 =>{
            debug!("addrconn cp_between got shutdown2 signal");

            let _ = tx1c.send(());
            let _ = tx2c.send(());

            0
        }
    };

    Ok(r)
}

#[cfg(test)]
mod test {
    use super::*;

    struct MyType {
        counter: u32,
    }
    impl crate::Name for MyType {
        fn name(&self) -> &str {
            "my_type"
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
