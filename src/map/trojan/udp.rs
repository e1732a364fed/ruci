use std::{
    cmp::max,
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
        addr_conn::{AsyncReadAddr, AsyncWriteAddr, MAX_DATAGRAM_SIZE},
        helpers::{self, MAX_LEN_SOCKS5_BYTES},
        Addr, Network,
    },
    utils::io_error,
};

use super::*;

//Reader 包装 ReadHalf<net::Conn>, 使其可以按trojan 格式读出 数据和Addr
pub struct Reader {
    pub base: Pin<Box<ReadHalf<net::Conn>>>,
    buf: BytesMut,
    state: ReadState,
    left_data_len: usize, //一个data包的 需要继续从base 读 的 剩余未读长读,
    old_ad: Addr,
}

impl Reader {
    pub fn new(r: ReadHalf<net::Conn>) -> Self {
        Self {
            base: Box::pin(r),
            buf: BytesMut::zeroed(MAX_DATAGRAM_SIZE),
            state: ReadState::Base,
            left_data_len: 0,
            old_ad: Addr::default(),
        }
    }
}

impl crate::Name for Reader {
    fn name(&self) -> &str {
        "trojan_udp(r)"
    }
}

enum ReadState {
    Base,
    Buf,
    LeftBuf,
}
impl Reader {
    fn poll_r(&mut self, cx: &mut Context<'_>) -> (Poll<io::Result<()>>, usize) {
        let mut tmp_rbuf = {
            let buffer = &mut self.buf;
            //buffer.clear();

            const TARGET_LEN: usize = MAX_DATAGRAM_SIZE / 2;
            // 每次 使用 buffer 都会 advance 导致 capcity 变小一部分
            // 如果每次都 resize 到最大, 则失去了使用同一个 缓存的意义
            // 故使用 一半最大. 这样 buffer 消耗到 一半之前是不会有 新 alloc 的

            if buffer.capacity() > TARGET_LEN {
                unsafe {
                    buffer.set_len(TARGET_LEN);
                }
            } else {
                buffer.resize(TARGET_LEN, 0)
            }

            ReadBuf::new(&mut buffer[..])
        };
        (
            self.base.as_mut().poll_read(cx, &mut tmp_rbuf),
            tmp_rbuf.filled().len(),
        )
    }
}

impl AsyncReadAddr for Reader {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        r_buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        //妥善处理 粘包, 短读 等情况

        loop {
            match self.state {
                ReadState::Base => {
                    //debug!("trojan read base");

                    let re = self.poll_r(cx);

                    //debug!("trojan reader read called");

                    match re.0 {
                        Poll::Pending => {
                            return Poll::Pending;
                        }

                        Poll::Ready(r) => match r {
                            Ok(_) => {
                                let data_len = re.1;
                                //debug!("trojan read base got {}", data_len);

                                if data_len == 0 {
                                    return Poll::Ready(Err(io::Error::new(
                                        io::ErrorKind::BrokenPipe,
                                        "trojan read base got 0",
                                    )));
                                } else {
                                    self.buf.truncate(data_len);

                                    if self.left_data_len > 0 {
                                        self.state = ReadState::LeftBuf;
                                    } else {
                                        self.state = ReadState::Buf;
                                    }
                                }
                            }
                            Err(e) => {
                                return Poll::Ready(Err(e));
                            }
                        },
                    }
                }
                ReadState::Buf => {
                    let buffer = &mut self.buf;
                    //debug!("trojan read buf {}", buffer.len());

                    let addr_r = helpers::socks5_bytes_to_addr(buffer);
                    match addr_r {
                        Ok(mut ad) => {
                            if buffer.len() < 2 {
                                buffer.clear();
                                self.state = ReadState::Base;
                                return Poll::Ready(Err(io::Error::other(
                                    "buf len short of data length part",
                                )));
                            }

                            let data_len = buffer.get_u16() as usize;
                            if buffer.len() - 2 < data_len {
                                let msg = format!(
                                    "buf len short of data , marked length+2:{}, real length: {}",
                                    data_len + 2,
                                    buffer.len()
                                );

                                buffer.clear();
                                self.state = ReadState::Base;
                                return Poll::Ready(Err(io::Error::other(msg)));
                            }
                            let crlf = buffer.get_u16();
                            if crlf != CRLF {
                                buffer.clear();
                                self.state = ReadState::Base;
                                return Poll::Ready(Err(io::Error::other(format!(
                                    "no crlf! {}",
                                    crlf
                                ))));
                            }

                            let buf_len = buffer.len();

                            let rbuf_len = r_buf.len();

                            let actual_read_len =
                                vec![data_len, rbuf_len, buf_len].into_iter().min().unwrap();

                            buffer.copy_to_slice(&mut r_buf[..actual_read_len]);
                            ad.network = Network::UDP;

                            // 123 132 213 231 312 321
                            //1: buf_len, 2: rbuf_len, 3: data_len

                            // 1. buf_len < rbuf_len < data_len : data > buffer, buffer < rbuf, need read base next
                            // 2. buf_len < data_len < rbuf_len : data > buffer, buffer < rbuf, need read base next
                            // 3. rbuf_len < buf_len < data_len : buf > rbuf && data > rbuf, need read buf next for left data
                            // 4. rbuf_len < data_len < buf_len : buf > rbuf && data > rbuf, need read buf next for left data
                            // 5. data_len < buf_len < rbuf_len : data is small; rbuf read ok; need read buf next
                            // 6. data_len < rbuf_len < buf_len : data is small; rbuf read ok; need read buf next

                            if (buf_len < rbuf_len) && (buf_len < data_len) {
                                self.left_data_len = data_len - rbuf_len;
                                self.state = ReadState::Base;
                            } else if (rbuf_len < buf_len) && (rbuf_len < data_len) {
                                self.left_data_len = data_len - rbuf_len;
                                self.old_ad = ad.clone();
                                self.state = ReadState::LeftBuf;
                            } else if (data_len < buf_len) && (data_len < rbuf_len) {
                                self.state = ReadState::Buf;
                            } else {
                                self.state = ReadState::Base;
                                self.left_data_len = 0;
                            }

                            return Poll::Ready(Ok((actual_read_len, ad)));
                        }
                        Err(e) => {
                            buffer.clear();
                            self.state = ReadState::Base;

                            return Poll::Ready(Err(io::Error::other(e)));
                        }
                    }
                }
                ReadState::LeftBuf => {
                    debug!("trojan read left buf {}", self.left_data_len);
                    let ldl = self.left_data_len;

                    let buffer = &mut self.buf;
                    let buf_len = buffer.len();

                    let rbuf_len = r_buf.len();

                    let to_read_len = vec![ldl, rbuf_len, buf_len].into_iter().min().unwrap();

                    buffer.copy_to_slice(&mut r_buf[..to_read_len]);

                    if (buf_len < rbuf_len) && (buf_len < ldl) {
                        self.state = ReadState::Base;
                    } else if (rbuf_len < buf_len) && (rbuf_len < ldl) {
                        self.left_data_len = ldl - rbuf_len;
                        self.state = ReadState::LeftBuf;
                    } else if (ldl < buf_len) && (ldl < rbuf_len) {
                        self.state = ReadState::Buf;
                    } else {
                        self.state = ReadState::Base;
                        self.left_data_len = 0;
                    }

                    return Poll::Ready(Ok((to_read_len, self.old_ad.clone())));
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

        match r {
            Poll::Pending => Poll::Pending,

            Poll::Ready(r) => match r {
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

    let ar = Reader::new(r);
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
