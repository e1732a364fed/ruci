/*!
 * 为 UdpSocket 实现 net::addr_conn 中的trait

tokio 中有 poll_recv_from 和 poll_send_to, 所以不用 再自行实现

async_std分支中，参考了

//https://users.rust-lang.org/t/implementing-custom-udp-trait-for-async-std/81000

*/
use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{BufMut, BytesMut};
use futures_util::Future;
use tokio::{io::ReadBuf, net::UdpSocket};

use self::addr_conn::{AddrConn, AddrReadTrait, AddrWriteTrait, AsyncReadAddr, AsyncWriteAddr};

use super::*;

/*

const CAP: usize = 256 * 256; //todo: change this

pub struct Writer {
    fut: Option<Pin<Box<dyn Future<Output = usize>>>>,

    //固定用同一个 udpsocket 发送，到不同的远程地址也是如此
    pub base: Arc<UdpSocket>,
}

//todo: remove all the unwraps to make it stable

impl AsyncWriteAddr for Writer {
    fn poll_write_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let mut fut = if let Some(fut) = self.fut.take() {
            fut
        } else {
            let waker = cx.waker().clone();
            let udp = self.base.clone();
            let adcopy = addr.clone();

            let mut buf2 = BytesMut::with_capacity(CAP);
            buf2.copy_from_slice(buf);

            Box::pin(async move {
                let n = udp
                    .send_to(&mut buf2, adcopy.get_socket_addr_or_resolve().unwrap())
                    .await
                    .unwrap();
                waker.wake();
                n
            })
        };

        match Pin::new(&mut fut).poll(cx) {
            Poll::Ready(res) => Poll::Ready(Ok(res)),
            Poll::Pending => {
                self.fut = Some(fut);
                Poll::Pending
            }
        }
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

pub struct Reader {
    fut: Option<Pin<Box<dyn Future<Output = (SocketAddr, BytesMut)>>>>,
    pub base: Arc<UdpSocket>,
}

impl AsyncReadAddr for Reader {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let mut fut = if let Some(fut) = self.fut.take() {
            fut
        } else {
            let waker = cx.waker().clone();
            let udp = self.base.clone();
            Box::pin(async move {
                let mut buf = BytesMut::with_capacity(CAP);
                let (_, from) = udp.recv_from(&mut buf).await.unwrap();
                waker.wake();
                (from, buf)
            })
        };

        match Pin::new(&mut fut).poll(cx) {
            Poll::Ready(res) => {
                let ad = Addr {
                    addr: NetAddr::Socket(res.0),
                    network: Network::UDP,
                };
                let mut len = buf.len();

                //res.1.copy_to_slice(buf);
                buf.put(res.1);

                len -= buf.len();

                Poll::Ready(Ok((len, ad)))
            }
            Poll::Pending => {
                self.fut = Some(fut);
                Poll::Pending
            }
        }
    }
}

pub fn get_rw(u: UdpSocket) -> (Reader, Writer) {
    let arc = Arc::new(u);

    let w = Writer {
        fut: None,
        base: arc.clone(),
    };
    let r = Reader {
        fut: None,
        base: arc,
    };

    (r, w)
}

use crate::net::addr_conn::AsyncReadAddrExt;
use crate::net::addr_conn::AsyncWriteAddrExt;

// Reader 和 Writer 没有实现 Unpin, 所以先如此写,  仿照 addr_conn 中 cp_addr 的代码

pub async fn cp_reader_to_w<W1: AddrWriteTrait>(mut r1: Reader, mut w1: W1) -> Result<u64, Error> {
    const CAP: usize = 1500;
    let mut inner = [0u8; CAP];
    let mut buf = ReadBuf::new(&mut inner);
    let mut whole_write = 0;
    loop {
        buf.clear();
        let r = r1.read(buf.initialized_mut()).await;
        if r.is_err() {
            break;
        }
        let (m, ad) = r.unwrap();
        if m > 0 {
            loop {
                let r = w1.write(&mut buf.filled(), &ad).await;
                if r.is_err() {
                    break;
                }
                let n = r.unwrap();
                buf.advance(n);
                if buf.filled().len() == 0 {
                    break;
                }
            }
        }
        whole_write += m;
    }

    Ok(whole_write as u64)
}

pub async fn cp_r_to_writer<R1: AddrReadTrait>(mut r1: R1, mut w1: Writer) -> Result<u64, Error> {
    const CAP: usize = 1500;
    let mut inner = [0u8; CAP];
    let mut buf = ReadBuf::new(&mut inner);

    let mut whole_write = 0;
    loop {
        buf.clear();
        let r = r1.read(buf.initialized_mut()).await;
        if r.is_err() {
            break;
        }
        let (m, ad) = r.unwrap();
        if m > 0 {
            loop {
                let r = w1.write(buf.filled_mut(), &ad).await;
                if r.is_err() {
                    break;
                }
                let n = r.unwrap();
                buf.advance(n);
                if buf.filled().len() == 0 {
                    break;
                }
            }
        }
        whole_write += m;
    }

    Ok(whole_write as u64)
}

pub async fn cp_addrconn_to_reader_writer<F: Fn() -> ()>(
    cid: u32,
    c1: AddrConn,
    r2: Reader,
    w2: Writer,
    shutdown_f: F,
    _opt: Option<Arc<TransmissionInfo>>,
) -> Result<u64, Error> {
    let (c1_to_c2, c2_to_c1) = (
        cp_r_to_writer(c1.0, w2).fuse(),
        cp_reader_to_w(r2, c1.1).fuse(),
    );
    pin_mut!(c1_to_c2, c2_to_c1);

    select! {
        r1 = c1_to_c2 =>{
            debug!("cid: {}, cp_addrconn_to_reader_writer end, r1",cid);
            shutdown_f();
            r1
        }
        r2 = c2_to_c1 =>{
            debug!("cid: {}, cp_addrconn_to_reader_writer end, r2",cid);

            shutdown_f();
            r2
        }
    }
}

pub async fn cp_addrconn_udpsocket<F: Fn() -> ()>(
    cid: u32,
    c1: AddrConn,
    c2: UdpSocket,
    shutdown_f: F,
    opt: Option<Arc<TransmissionInfo>>,
) -> Result<u64, Error> {
    let (r, w) = get_rw(c2);
    cp_addrconn_to_reader_writer(cid, c1, r, w, shutdown_f, opt).await
}

#[allow(unused)]
mod test {
    use udp::addr_conn::{AddrReadTrait, AsyncReadAddrExt};

    use super::*;
    use std::io;

    async fn t1() -> io::Result<()> {
        let u = UdpSocket::bind("127.0.0.1:0").await?;

        let (r, w) = get_rw(u);

        //let mut buf = BytesMut::zeroed(CAP);
        let mut x = [0u8; CAP];
        let mut buf = ReadBuf::new(&mut x);
        //futures::pin_mut!(r);
        use std::pin::pin;
        let mut r = pin!(r);
        r.read(buf.initialized_mut());

        unimplemented!()
    }
}

 */
