use std::{
    cmp::min,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, BufMut, BytesMut};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf, ReadHalf, WriteHalf};

use crate::net::{
    self,
    addr_conn::{AsyncReadAddr, AsyncWriteAddr},
    helpers::{self, MAX_LEN_SOCKS5_BYTES},
    Addr, Network,
};

use super::*;

//Reader 包装 ReadHalf<net::Conn>，使其可以按trojan 格式读出 数据和Addr
pub struct Reader {
    pub base: Pin<Box<ReadHalf<net::Conn>>>,
}

impl crate::Name for Reader {
    fn name(&self) -> &str {
        "trojan_udp(r)"
    }
}

impl AsyncReadAddr for Reader {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut r_buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let mut inner = BytesMut::zeroed(r_buf.len() + helpers::MAX_LEN_SOCKS5_BYTES);
        let mut buf2 = ReadBuf::new(&mut inner[..]);

        let r = self.base.as_mut().poll_read(cx, &mut buf2);

        match r {
            Poll::Ready(re) => {
                match re {
                    Ok(_) => {
                        let mut buf2 = inner;

                        let addr_r = helpers::socks5_bytes_to_addr(&mut buf2);
                        match addr_r {
                            Ok(mut ad) => {
                                if buf2.len() < 2 {
                                    return Poll::Ready(Err(io::Error::other(
                                        "buf len short of data length part",
                                    )));
                                }

                                let l = buf2.get_u16() as usize;
                                if buf2.len() - 2 < l {
                                    return Poll::Ready(Err(io::Error::other(format!("buf len short of data , marked length+2:{}, real length: {}", l+2, buf2.len()))));
                                }
                                let crlf = buf2.get_u16();
                                if crlf != CRLF {
                                    return Poll::Ready(Err(io::Error::other(format!(
                                        "no crlf! {}",
                                        crlf
                                    ))));
                                }
                                buf2.truncate(l);

                                let real_len = min(l, r_buf.len());

                                r_buf.put(&buf2[..real_len]);
                                ad.network = Network::UDP;

                                Poll::Ready(Ok((real_len, ad)))
                            }
                            Err(e) => Poll::Ready(Err(io::Error::other(e))),
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
impl crate::Name for Writer {
    fn name(&self) -> &str {
        "trojan_udp(w)"
    }
}

impl AsyncWriteAddr for Writer {
    fn poll_write_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let mut buf2 = BytesMut::with_capacity(MAX_LEN_SOCKS5_BYTES + buf.len());

        helpers::addr_to_socks5_bytes(addr, &mut buf2);

        buf2.put_u16(buf.len() as u16);
        buf2.put_u16(CRLF);
        buf2.put(buf);

        self.base.as_mut().poll_write(cx, &buf2)
    }

    fn poll_flush_addr(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.base.as_mut().poll_flush(cx)
    }

    fn poll_close_addr(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.base.as_mut().poll_shutdown(cx)
    }
}

pub fn from(c: net::Conn) -> net::addr_conn::AddrConn {
    let (r, w) = tokio::io::split(c);

    let ar = Reader { base: Box::pin(r) };
    let aw = Writer { base: Box::pin(w) };

    net::addr_conn::AddrConn::new(Box::new(ar), Box::new(aw))
}

#[cfg(test)]
mod test {

    use self::net::{
        addr_conn::{AsyncReadAddrExt, AsyncWriteAddrExt},
        helpers::MockTcpStream,
    };
    use super::*;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_w() -> std::io::Result<()> {
        let writev = Arc::new(Mutex::new(Vec::new()));
        let writevc = writev.clone();

        let cs = MockTcpStream {
            read_data: Vec::new(),
            write_data: Vec::new(),
            write_target: Some(writev),
        };

        let conn: net::Conn = Box::new(cs);

        let (_r, w) = tokio::io::split(conn);

        let mut aw = Writer { base: Box::pin(w) };

        let ad = net::Addr {
            network: net::Network::UDP,
            addr: net::NetAddr::Name("www.b".to_string(), 43),
        };

        let r = aw.write(&b"hello"[..], &ad).await;

        println!("r, {:?}, writev {:?}", r, writevc);

        Ok(())
    }

    #[tokio::test]
    async fn test_r1() -> std::io::Result<()> {
        test_r_with_buflen(1024).await
    }

    #[tokio::test]
    async fn test_r2_short() -> std::io::Result<()> {
        test_r_with_buflen(2).await
    }

    async fn test_r_with_buflen(rbuflen: usize) -> std::io::Result<()> {
        let cs = MockTcpStream {
            read_data: vec![
                3, 5, 119, 119, 119, 46, 98, 0, 43, 0, 5, 13, 10, 104, 101, 108, 108, 111,
            ], //www.b:43, hello
            write_data: Vec::new(),
            write_target: None,
        };

        let conn: net::Conn = Box::new(cs);

        let (r, _w) = tokio::io::split(conn);

        let mut ar = Reader { base: Box::pin(r) };

        let ad = net::Addr {
            network: net::Network::UDP,
            addr: net::NetAddr::Name("www.b".to_string(), 43),
        };

        let mut buf = BytesMut::zeroed(rbuflen); //[0u8; rbuflen];

        let r = ar.read(&mut buf).await;

        println!("r, {:?},  ", r,);
        if let Ok((l, addr)) = r {
            println!("a,b, {:?},{:?},{:?},  ", l, addr, &buf[..l]);

            assert_eq!(addr, ad);
        }

        Ok(())
    }
}
