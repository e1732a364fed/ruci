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

// todo improve the codes below

async fn read_once_notimeout<R1: AddrReadTrait, W1: AddrWriteTrait>(
    r1: &mut R1,
    w1: &mut W1,
    buf: &mut [u8],
) -> bool {
    match r1.read(buf).await {
        Err(e) => {
            debug!("read addrconn GOT ERROR {e}");

            return false;
        }
        Ok((m, ad)) => {
            if m > 0 {
                let r = w1.write(&buf[..m], &ad).await;
                match r {
                    Ok(_) => return true,
                    Err(e) => {
                        debug!("write addrconn err {e}");

                        return false;
                    }
                }
            }
            return false;
        }
    }
}

async fn read_once<R1: AddrReadTrait, W1: AddrWriteTrait>(
    r1: &mut R1,
    w1: &mut W1,
    no_timeout: bool,
    buf: &mut [u8],
) -> bool {
    if no_timeout {
        return read_once_notimeout(r1, w1, buf).await;
    }

    tokio::select! {
        _ = tokio::time::sleep(CP_UDP_TIMEOUT) =>{
            debug!("read addrconn timeout");

            return false ;
        }

        r = r1.read(buf) =>{
            match r {
                Err(e) => {
                    debug!("read addrconn GOT ERROR {e}");

                    return false
                },
                Ok((m, ad)) => {
                    if m > 0 {
                        //写udp 是不会卡住的, 但addr_conn底层可能不是 udp

                        let sleep_f2 = tokio::time::timeout(Duration::from_secs(1),w1.write(&buf[..m], &ad)).await;
                        match sleep_f2{
                            Ok(r) => match r {
                                Ok(_) => return true,
                                Err(e) => {
                                    debug!("write addrconn err {e}");

                                    return false},
                            },
                            Err(_) => {
                                debug!("write addrconn timeout");
                                return true;
                            },
                        }

                    }
                    return false ;
                }
            }
        }
    } //select
}

/// 循环读写直到read错误发生. 不会认为 read错误为错误. 每一次read都会以
/// CP_UDP_TIMEOUT 为 最长等待时间, 一旦读不到, 就会退出函数
///
/// 读到后, 如果写超过了同样的时间, 也退出函数
pub async fn cp_addr<R1: AddrReadTrait + 'static, W1: AddrWriteTrait + 'static>(
    mut r1: R1,
    mut w1: W1,
    _name: String,
    no_timeout: bool,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<u64, Error> {
    //let mut whole_write = 0;
    let mut buf0 = Box::new([0u8; 1400]);

    loop {
        tokio::select! {
            r = read_once(&mut r1, &mut w1, no_timeout,buf0.as_mut()) =>{
                if !r {
                    break;
                }
            }
            _ = shutdown_rx.recv() =>{
                break;
            }
        }
    } //loop

    // 实测 用一个loop + 小 buf 的实现 比用 两个 spawn + mpsc 快很多. 后者非常卡顿几乎不可用
    // buf size 的选择也很重要, 太大太小都卡

    // let (tx, mut rx) = mpsc::channel(1000);

    // let jh1 = tokio::spawn(async move {
    //     loop {
    //         let mut buf = BytesMut::zeroed(1500);
    //         let r = r1.read(buf.as_mut()).await;
    //         match r {
    //             Ok((u, a)) => {
    //                 buf.truncate(u);
    //                 let r = tx.send((buf, a)).await;
    //                 if r.is_err() {
    //                     break;
    //                 }
    //             }
    //             Err(_e) => {
    //                 break;
    //             }
    //         }
    //     }
    // });

    // let jh2 = tokio::spawn(async move {
    //     loop {
    //         let r = rx.recv().await;
    //         match r {
    //             Some((buf, a)) => {
    //                 let r = w1.write(&buf, &a).await;
    //                 if r.is_err() {
    //                     break;
    //                 }
    //             }
    //             None => break,
    //         }
    //     }
    // });

    // let _ = join!(jh1, jh2);

    // debug!("CP ADDR exit {name}  {whole_write}");
    // Ok(whole_write as u64)
    Ok(0)
}

/// copy data between two AddrConn struct
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
    _opt: Option<Arc<GlobalTrafficRecorder>>,
    no_timeout: bool,
    shutdown_rx1: Option<tokio::sync::oneshot::Receiver<()>>,
    shutdown_rx2: Option<tokio::sync::oneshot::Receiver<()>>,
) -> Result<u64, Error> {
    //let (tx1, rx1) = oneshot::channel();
    //let (tx2, rx2) = oneshot::channel();
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

    let cp1 = tokio::spawn(cp_addr(r1, w2, n1, no_timeout, rx1));
    let cp2 = tokio::spawn(cp_addr(r2, w1, n2, no_timeout, rx2));

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

             debug!("shutdown1 ended");
            // rst2
            0
        }

        _ = shutdown_rx2 =>{
            debug!("addrconn cp_between got shutdown2 signal");

            let _ = tx1c.send(());
            let _ = tx2c.send(());

             debug!("shutdown2 ended");
            // rst2
            0
        }
    };

    Ok(r)

    // unimplemented!()

    // let (c1_to_c2, c2_to_c1) = (
    //     cp_addr(r1, w2, no_timeout, rx1).fuse(),
    //     cp_addr(r2, w1, no_timeout, rx2).fuse(),
    // );
    // pin_mut!(c1_to_c2, c2_to_c1, shutdown_rx1, shutdown_rx2);

    // futures::select! {
    //     _ = shutdown_rx1 =>{
    //         debug!("addrconn cp_between got shutdown1 signal");

    //         let _ = tx1.send(());
    //         let _ = tx2.send(());

    //         let _rst1 = c1_to_c2.await;
    //         let rst2 = c2_to_c1.await;

    //         debug!("addrconn cp_between ended");
    //         rst2
    //     }

    //     _ = shutdown_rx2 =>{
    //         debug!("addrconn cp_between got shutdown2 signal");

    //         let _ = tx1.send(());
    //         let _ = tx2.send(());

    //         let _rst1 = c1_to_c2.await;
    //         let rst2 = c2_to_c1.await;

    //         debug!("addrconn cp_between ended");
    //         rst2
    //     }
    //     rst1 = c1_to_c2 =>{

    //         if tracing::enabled!(tracing::Level::DEBUG)  {
    //             debug!( cid = %cid,"cp_addr end, u");
    //         }

    //         if let Some(gtr) = opt.as_ref(){
    //             match &rst1{
    //                 Ok(n) => {
    //                     let tt = gtr.ub.fetch_add(*n, Ordering::Relaxed);
    //                     debug!(cid = %cid,"cp_addr_end, u, ub, {},{}",n,tt+n);
    //                 },
    //                 Err(e) => {
    //                     debug!(cid = %cid,"cp_addr_end with err, u, {}",e);

    //                 },
    //             }

    //         }
    //         let _ = tx2.send(());

    //         let rst2 = c2_to_c1.await;

    //         if tracing::enabled!(tracing::Level::DEBUG)  {
    //             debug!(cid = %cid,"cp_addr end, d");
    //         }

    //         if let Some(gtr) = opt{
    //             match &rst2{
    //                 Ok(n) => {
    //                     let tt = gtr.db.fetch_add(*n, Ordering::Relaxed);
    //                     debug!(cid = %cid,"cp_addr_end, u ,db, {},{}",n,tt+n);
    //                 },
    //                 Err(e) => {
    //                     debug!(cid = %cid,"cp_addr_end with err, u, d, {}",e);

    //                 },
    //             }
    //         }

    //         rst1
    //     }
    //     rst2 = c2_to_c1 =>{
    //         if tracing::enabled!(tracing::Level::DEBUG)  {
    //             debug!(cid = %cid,"cp_addr end, d");
    //         }
    //         if let Some(gtr) = opt.as_ref(){
    //             match &rst2{
    //                 Ok(n) => {
    //                     let tt = gtr.db.fetch_add(*n, Ordering::Relaxed);
    //                     debug!(cid = %cid,"cp_addr_end, d, db, {},{}",n,tt+n);
    //                 },
    //                 Err(e) => {
    //                     debug!(cid = %cid,"cp_addr_end with err, d, d, {}",e);

    //                 },
    //             }

    //         }
    //         let _ = tx1.send(());

    //         let rst1 = c1_to_c2.await;
    //         if tracing::enabled!(tracing::Level::DEBUG)  {
    //             debug!(cid = %cid,"cp_addr end, u, ");
    //         }
    //         if let Some(gtr) = opt{

    //             match &rst1{
    //                 Ok(n) => {
    //                     let tt = gtr.ub.fetch_add(*n, Ordering::Relaxed);
    //                     debug!(cid = %cid,"cp_addr_end, d, ub, {},{}",n,tt+n);
    //                 },
    //                 Err(e) => {
    //                     debug!(cid = %cid,"cp_addr_end with err, d, u, {}",e);

    //                 },
    //             }
    //         }
    //         rst2
    //     }
    // }
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
