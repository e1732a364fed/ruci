use std::{
    cmp::{max, min},
    io,
    pin::Pin,
    task::{ready, Context, Poll},
};

use bytes::{Buf, BufMut, BytesMut};
use fmt::Display;
use tokio::io::{AsyncWrite, ReadHalf, WriteHalf};
use tracing::debug;

use crate::{
    map::helpers::ContentLenProtocolBufReader,
    net::{
        self,
        addr_conn::{AsyncReadAddr, AsyncWriteAddr, MAX_DATAGRAM_SIZE},
        helpers::{self, MAX_LEN_SOCKS5_BYTES},
        Addr,
    },
    utils::io_error,
};

use super::*;

//Reader 包装 ReadHalf<net::Conn>, 使其可以按trojan 格式读出 数据和Addr
pub struct Reader {
    reader: ContentLenProtocolBufReader,
}

impl Reader {
    pub fn new(r: ReadHalf<net::Conn>) -> Self {
        Self {
            reader: ContentLenProtocolBufReader::new(
                MAX_DATAGRAM_SIZE,
                Box::pin(r),
                Box::new(|data: &[u8]| {
                    // 解析头部,返回(content_len, body_start_index)
                    let mut buf = BytesMut::from(data);

                    if buf.len() < 4 {
                        // 2字节长度 + 2字节CRLF
                        return Err(io::Error::other("insufficient header length"));
                    }

                    if let Err(e) = helpers::socks5_bytes_to_addr(&mut buf) {
                        return Err(io::Error::other(e));
                    }

                    let data_len = buf.get_u16() as usize;
                    let crlf = buf.get_u16();
                    if crlf != CRLF {
                        return Err(io::Error::other("invalid CRLF"));
                    }
                    Ok(helpers::ContentLenProtocolPacketMetadata {
                        content_len: data_len,
                        body_start_index: data.len() - buf.len(),
                    })
                }),
            ),
        }
    }
}

impl Display for Reader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "trojan_udp(r)")
    }
}

impl AsyncReadAddr for Reader {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        r_buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        match ready!(self.reader.read(cx)) {
            Ok(Some(result)) => {
                let len = min(result.body_to - result.body_from, r_buf.len());
                r_buf[..len].copy_from_slice(&result.buf[result.body_from..result.body_from + len]);

                // 解析地址
                let mut a_buf = BytesMut::from(&result.buf[..result.body_from]);
                let addr = helpers::socks5_bytes_to_addr(&mut a_buf);

                match addr {
                    Ok(mut addr) => {
                        addr.network = net::Network::UDP;
                        self.reader.put_back(result.buf);
                        Poll::Ready(Ok((len, addr)))
                    }
                    Err(e) => Poll::Ready(Err(io::Error::other(e))),
                }
            }
            Ok(None) => Poll::Ready(Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"))),
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

//Writer 包装 WriteHalf<net::Conn>, 使其可以按trojan 格式写入 数据和Addr
pub struct Writer {
    pub base: Pin<Box<WriteHalf<net::Conn>>>,

    pub last_buf: Option<BytesMut>,
}
impl Display for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "trojan_udp(w)")
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
                b.reserve(max(MAX_DATAGRAM_SIZE, supposed_cap));
            }

            b
        } else {
            BytesMut::with_capacity(MAX_DATAGRAM_SIZE)
        };

        helpers::addr_to_socks5_bytes(addr, &mut buf2);

        let data_l = buf.len();

        buf2.put_u16(data_l as u16);
        buf2.put_u16(CRLF);
        buf2.put(buf);

        let actual_l = buf2.len();

        let r = self.base.as_mut().poll_write(cx, &buf2);
        //debug!("trojan writer write got {data_l} {actual_l} {:?}", r);

        buf2.clear();
        self.last_buf = Some(buf2);

        match ready!(r) {
            Ok(n) => match n.cmp(&actual_l) {
                std::cmp::Ordering::Less => {
                    let diff = actual_l - n;
                    debug!(
                        "trojan writer write got short write {} {} {}",
                        actual_l, n, diff
                    );

                    Poll::Ready(Ok(data_l - diff))
                }
                std::cmp::Ordering::Equal => Poll::Ready(Ok(data_l)),
                std::cmp::Ordering::Greater => Poll::Ready(Err(io_error(format!(
                    "trojan udp write got impossible n > actual_l, {} {}",
                    n, actual_l
                )))),
            },
            Err(e) => Poll::Ready(Err(e)),
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

    let ar = Reader::new(r);
    let aw = Writer::new(w);

    net::addr_conn::AddrConn::new(Box::new(ar), Box::new(aw))
}

#[cfg(test)]
mod test {

    use self::net::{
        addr_conn::{AsyncReadAddrExt, AsyncWriteAddrExt},
        helpers::mock::MockTcpStream,
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

        let mut ar = Reader::new(r);

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
