use std::{
    cmp::min,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, BufMut, BytesMut};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf, ReadHalf, WriteHalf};
use tracing::debug;

use crate::{
    net::{
        self,
        addr_conn::{AsyncReadAddr, AsyncWriteAddr},
        helpers::{self, MAX_LEN_SOCKS5_BYTES},
        Addr, Network,
    },
    utils::io_error,
};

use super::*;

//Reader 包装 ReadHalf<net::Conn>, 使其可以按trojan 格式读出 数据和Addr
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
        r_buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let mut inner = BytesMut::zeroed(r_buf.len() + helpers::MAX_LEN_SOCKS5_BYTES);
        let mut buf2 = ReadBuf::new(&mut inner[..]);

        //debug!("trojan reader read called");

        let r = self.base.as_mut().poll_read(cx, &mut buf2);

        //debug!("trojan reader read got {:?}", r);

        match r {
            Poll::Pending => Poll::Pending,

            Poll::Ready(re) => {
                match re {
                    Ok(_) => {
                        //debug!("trojan reader read got len {:?}", buf2.filled().len());

                        let mut the_buf2 = inner;

                        let addr_r = helpers::socks5_bytes_to_addr(&mut the_buf2);
                        match addr_r {
                            Ok(mut ad) => {
                                if the_buf2.len() < 2 {
                                    return Poll::Ready(Err(io::Error::other(
                                        "buf len short of data length part",
                                    )));
                                }

                                let data_len = the_buf2.get_u16() as usize;
                                if the_buf2.len() - 2 < data_len {
                                    return Poll::Ready(Err(io::Error::other(format!("buf len short of data , marked length+2:{}, real length: {}", data_len+2, the_buf2.len()))));
                                }
                                let crlf = the_buf2.get_u16();
                                if crlf != CRLF {
                                    return Poll::Ready(Err(io::Error::other(format!(
                                        "no crlf! {}",
                                        crlf
                                    ))));
                                }
                                the_buf2.truncate(data_len);

                                let real_len = min(data_len, r_buf.len());
                                debug!("trojan reader read got real_len {:?}", real_len);

                                the_buf2.copy_to_slice(&mut r_buf[..real_len]);
                                //r_buf.put(&the_buf2[..real_len]);
                                ad.network = Network::UDP;

                                Poll::Ready(Ok((real_len, ad)))
                            }
                            Err(e) => Poll::Ready(Err(io::Error::other(e))),
                        }
                    }
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
        }
    }
}

//Writer 包装 WriteHalf<net::Conn>, 使其可以按trojan 格式写入 数据和Addr
pub struct Writer {
    pub base: Pin<Box<WriteHalf<net::Conn>>>,

    pub last_buf: Option<BytesMut>,
}
impl crate::Name for Writer {
    fn name(&self) -> &str {
        "trojan_udp(w)"
    }
}
impl Writer {
    pub fn new(base: WriteHalf<net::Conn>) -> Self {
        Self {
            base: Box::pin(base),
            last_buf: None,
        }
    }
}

impl AsyncWriteAddr for Writer {
    fn poll_write_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        // debug!("trojan writer called {}", buf.len());
        let supposed_cap = MAX_LEN_SOCKS5_BYTES + buf.len();
        let mut buf2 = if let Some(mut b) = self.last_buf.take() {
            let c = b.capacity();
            if c < supposed_cap {
                b.reserve(supposed_cap);
            }

            b
        } else {
            BytesMut::with_capacity(supposed_cap)
        };

        helpers::addr_to_socks5_bytes(addr, &mut buf2);

        let data_l = buf.len();

        buf2.put_u16(data_l as u16);
        buf2.put_u16(CRLF);
        buf2.put(buf);

        let actual_l = buf2.len();

        let r = self.base.as_mut().poll_write(cx, &buf2);
        debug!("trojan writer write got {data_l} {actual_l} {:?}", r);

        buf2.clear();
        self.last_buf = Some(buf2);

        match r {
            Poll::Pending => {
                return Poll::Pending;
            }

            Poll::Ready(r) => match r {
                Ok(n) => {
                    if n == actual_l {
                        return Poll::Ready(Ok(data_l));
                    } else {
                        if n > actual_l {
                            return Poll::Ready(Err(io_error(format!(
                                "trojan udp write got impossible n > actual_l, {} {}",
                                n, actual_l
                            ))));
                        } else {
                            let diff = actual_l - n;
                            debug!(
                                "trojan writer write got short write {} {} {}",
                                actual_l, n, diff
                            );

                            return Poll::Ready(Ok(data_l - diff));
                        }
                    }
                }
                Err(e) => return Poll::Ready(Err(e)),
            },
        }
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
    let aw = Writer::new(w);

    let mut ac = net::addr_conn::AddrConn::new(Box::new(ar), Box::new(aw));

    ac.cached_name = String::from("trojan_udp");
    ac
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

        let mut aw = Writer::new(w);

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
