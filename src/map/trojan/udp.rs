use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, BufMut, BytesMut};
use futures_util::io::{ReadHalf, WriteHalf};

use crate::net::{
    self,
    addr_conn::{AsyncReadAddr, AsyncWriteAddr},
    helpers, Addr,
};

use super::*;
use futures::AsyncRead;
use futures::AsyncWrite;

const CAP: usize = 256 * 256; //todo: change this

//Reader 包装 ReadHalf<net::Conn>，使其可以按trojan 格式读出 数据和Addr
pub struct Reader {
    pub base: Pin<Box<ReadHalf<net::Conn>>>,
}

impl AsyncReadAddr for Reader {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let mut buf2 = BytesMut::with_capacity(CAP);

        let r = self.base.as_mut().poll_read(cx, &mut buf2);

        match r {
            Poll::Ready(re) => {
                match re {
                    Ok(_) => {
                        let x = helpers::socks5_bytes_to_addr(&mut buf2);
                        match x {
                            Ok(ad) => {
                                if buf2.len() < 2 {
                                    return Poll::Ready(Err(io::Error::other(
                                        "buf len short of data lenth part",
                                    )));
                                }

                                let l = buf2.get_u16() as usize;
                                if buf2.len() - 2 < l {
                                    return Poll::Ready(Err(io::Error::other(format!("buf len short of data , marked lenth+2:{}, real length: {}", l+2, buf2.len()))));
                                }
                                let crlf = buf2.get_u16();
                                if crlf != CRLF {
                                    return Poll::Ready(Err(io::Error::other(format!(
                                        "no crlf! {}",
                                        crlf
                                    ))));
                                }
                                buf2.truncate(l);

                                buf2.copy_to_slice(buf);

                                Poll::Ready(Ok((l, ad)))
                            }
                            Err(e) => Poll::Ready(Err(e)),
                        }
                    }
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

//Writer 包装 WriteHalf<net::Conn>，使其可以按trojan 格式写入 数据和Addr
pub struct Writer {
    pub base: Pin<Box<WriteHalf<net::Conn>>>,
}

impl AsyncWriteAddr for Writer {
    fn poll_write_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: Addr,
    ) -> Poll<io::Result<usize>> {
        let mut buf2 = BytesMut::with_capacity(CAP);

        helpers::addr_to_socks5_bytes(addr, &mut buf2);

        buf2.put_u16(buf.len() as u16);

        buf2.put_u16(CRLF);

        let r = self.base.as_mut().poll_write(cx, &buf2);
        r
    }

    fn poll_flush_addr(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.base.as_mut().poll_flush(cx)
    }

    fn poll_close_addr(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.base.as_mut().poll_close(cx)
    }
}

pub fn split_conn_to_trojan_udp_rw(c: net::Conn) -> net::addr_conn::AddrConn {
    use futures::AsyncReadExt;
    let (r, w) = c.split();

    let ar = Reader { base: Box::pin(r) };
    let aw = Writer { base: Box::pin(w) };

    net::addr_conn::AddrConn(Box::new(ar), Box::new(aw))
}

#[allow(unused)]
#[cfg(test)]
mod test {
    use futures_util::AsyncReadExt;

    use self::net::addr_conn::AsyncWriteAddrExt;

    use super::*;

    //#[async_test]
    async fn test1() -> std::io::Result<()> {
        let ps = net::gen_random_higher_port();
        let listen_host = "127.0.0.1";
        let listen_port = ps;

        let cs = TcpStream::connect((listen_host, listen_port))
            .await
            .unwrap();

        let conn: net::Conn = Box::new(cs);

        let (r, w) = conn.split();

        let mut ar = Reader { base: Box::pin(r) };

        let mut aw = Writer { base: Box::pin(w) };

        let r = aw.write(&b"hello"[..], net::Addr::default()).await;

        Ok(())
    }
}
