use crate::Name;

use super::*;

use core::time;
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::oneshot;
use tracing::info;

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
impl Debug for AddrConn {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddrConn")
            .field("default_write_to", &self.default_write_to)
            .field("cached_name", &self.cached_name)
            .finish()
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

pub const CP_UDP_TIMEOUT: time::Duration = Duration::from_secs(100); //todo: adjust this
pub const MAX_DATAGRAM_SIZE: usize = 65535 - 20 - 8;
pub const MTU: usize = 1400;

async fn rw_once<R: AddrReadTrait, W: AddrWriteTrait>(
    r: &mut R,
    w: &mut W,
    buf: &mut [u8],
) -> io::Result<usize> {
    let (rn, a) = r.read(buf).await?;

    let wn = w.write(&buf[..rn], &a).await?;

    let r = w.flush().await;
    match r {
        Ok(_) => Ok(wn),
        Err(e) => Err(e),
    }
}

/// 循环读写直到read错误发生. 不会认为 read错误为错误. 每一次read都会以
/// CP_UDP_TIMEOUT 为 最长等待时间, 一旦读不到, 就会退出函数
///
/// 读到后, 如果写超过了同样的时间, 也退出函数
pub async fn cp_addr<R: AddrReadTrait + 'static, W: AddrWriteTrait + 'static>(
    mut r: R,
    mut w: W,
    name: String,
    no_timeout: bool,
    mut shutdown_rx: oneshot::Receiver<()>,
    is_d: bool,
    opt: Option<Arc<GlobalTrafficRecorder>>,
) -> Result<u64, Error> {
    // 实测 用一个loop + 小 buf 的实现 比用 两个 spawn + mpsc 快很多. 后者非常卡顿几乎不可用
    // buf size 的选择也很重要, 太大太小都卡

    let mut whole_write = 0;
    let mut buf = Box::new([0u8; MTU]);

    loop {
        tokio::select! {
            r = rw_once(&mut r, &mut w, buf.as_mut()) =>{
                match r {
                    Ok(n) => whole_write+=n,
                    Err(e) => {
                        match e.kind(){
                            io::ErrorKind::Other => {
                                debug!("cp_addr got other e, will continue: {e}");
                                continue;
                            },
                            _ => {
                                // udp timeout 时 常会发生, 因此不能认为是错误
                                info!(name = name,"cp_addr got e, will break: {e}");
                            },
                        }

                        break
                    },
                }
            }
            _ = async{
                if no_timeout{
                    std::future::pending().await
                }else{
                    tokio::time::sleep(CP_UDP_TIMEOUT).await
                }
            } =>{
                info!(timeout = ?CP_UDP_TIMEOUT,"cp_addr got timeout, will break");

                break;
            }

            _ = &mut shutdown_rx =>{
                info!("cp_addr got shutdown_rx, will break");

                break;
            }
        }
        tokio::task::yield_now().await; //necessary, or it is likely to cause stuck issue
    } //loop

    let l = whole_write as u64;
    if let Some(a) = opt {
        if is_d {
            a.db.fetch_add(l, Ordering::Relaxed);
        } else {
            a.ub.fetch_add(l, Ordering::Relaxed);
        }
    }
    let _ = w.shutdown().await;

    Ok(l)
}

/// copy data between two [`AddrConn`] struct
///
/// blocking
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
    let n1 = c1.cached_name.clone() + " to " + &c2.cached_name;
    let n2 = c2.cached_name.clone() + " to " + &c1.cached_name;

    let (tx1, rx1) = oneshot::channel();
    let (tx2, rx2) = oneshot::channel();

    let cp1 = tokio::spawn(cp_addr(c1.r, c2.w, n1, no_timeout, rx1, false, opt.clone()));
    let cp2 = tokio::spawn(cp_addr(c2.r, c1.w, n2, no_timeout, rx2, true, opt.clone()));

    if let Some(gtr) = &opt {
        gtr.alive_connection_count.fetch_add(1, Ordering::Relaxed);
    }

    let r = tokio::select! {
        r = cp1 =>{
            if tracing::enabled!(tracing::Level::DEBUG)  {
                debug!( cid = %cid,"addr_conn::cp end, u");
            }
            let _ = tx1.send(());
            let _ = tx2.send(());

            match r{
                Ok(r) => r.unwrap_or(0),
                Err(_) => 0,
            }
        }
        r = cp2 =>{
            if tracing::enabled!(tracing::Level::DEBUG)  {
                debug!( cid = %cid,"addr_conn::cp end, d");
            }
            let _ = tx1.send(());
            let _ = tx2.send(());

            match r{
                Ok(r) => r.unwrap_or(0),
                Err(_) => 0,
            }
        }
        _ = async{
            if let Some(shutdown_rx1) = shutdown_rx1{
                shutdown_rx1.await
            }else{
                std::future::pending().await
            }
        } =>{
            debug!("addrconn cp_between got shutdown1 signal");

            let _ = tx1.send(());
            let _ = tx2.send(());

            0
        }

        _ = async{
            if let Some(shutdown_rx2) = shutdown_rx2{
                shutdown_rx2.await
            }else{
                std::future::pending().await
            }
        } =>{
            debug!("addrconn cp_between got shutdown2 signal");
            let _ = tx1.send(());
            let _ = tx2.send(());

            0
        }
    };

    if let Some(gtr) = opt {
        gtr.alive_connection_count
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
    info!( cid = %cid, "cp_addr_conn end" );

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
