use crate::{
    net::{self, Stream, CID},
    Name,
};
use async_trait::async_trait;
use bytes::BytesMut;
use std::{io, pin::Pin, task::Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::*;

/// 按字节加法器。
///
/// 本端write 被调用时 进行输入, 另一端read  被调用时进行输出,
///
/// 输出为输入按字节+(add)
///
/// 例子： add 为1 时, 若read 到的值是 [1,2,3], 则将向外输出 [2,3,4]
///
/// 伪代码示例：
///
///
/// let lbuf = [0u8,1,2,3];
/// let rbuf = [0u8,0,0,0];
/// l_adder_conn.write(&lbuf).await;
/// r_any_conn.read(&rbuf).await;
/// assert_eq!([1,2,3,4],rbuf);
///
/// 本端read按原值返回 base 的read的值
///
///
pub struct AdderConn {
    pub add: i8,
    pub cid: CID,
    base: Pin<net::Conn>,
    wbuf: BytesMut,

    readl: usize,

    direction: AddDirection,
}

//todo: 考虑使用 simd 或 rayon; 可以在其它impl包中实现, 也可在此实现

#[derive(Clone, Copy, Debug, Default)]
pub enum AddDirection {
    #[default]
    Read,
    Write,
    Both,
}

impl AdderConn {
    //write self.wbuf to  self.base
    fn write_wbuf(&mut self, cx: &mut std::task::Context<'_>) -> Poll<io::Result<usize>> {
        self.base.as_mut().poll_write(cx, &self.wbuf)
    }

    //read self.base to self.wbuf
    fn read_wbuf(&mut self, cx: &mut std::task::Context<'_>) -> Poll<io::Result<()>> {
        let mut abuf = &mut self.wbuf;
        abuf.resize(abuf.capacity(), 0);
        let mut rb = ReadBuf::new(&mut abuf);

        let r = self.base.as_mut().poll_read(cx, &mut rb);

        self.readl = rb.filled().len();
        self.wbuf.resize(self.readl, 0);

        let x: i16 = self.add as i16;
        for a in self.wbuf.iter_mut() {
            *a = (x + *a as i16) as u8;
        }

        r
    }
}
impl Name for AdderConn {
    fn name(&self) -> &'static str {
        "adder_conn"
    }
}

#[test]
fn set_size_tocap() {
    let mut bytes_mut = BytesMut::with_capacity(10);

    // 设置 size 等于 cap
    bytes_mut.resize(bytes_mut.capacity(), 0);
    assert_eq!(bytes_mut.len(), bytes_mut.capacity());
}

impl AsyncRead for AdderConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.direction {
            AddDirection::Write => self.base.as_mut().poll_read(cx, buf),
            _ => {
                let r = self.read_wbuf(cx);

                if let Poll::Ready(Ok(_)) = &r {
                    buf.put_slice(&self.wbuf);
                }
                r
            }
        }
    }
}

impl AsyncWrite for AdderConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.direction {
            AddDirection::Read => self.base.as_mut().poll_write(cx, buf),
            _ => {
                let x: i16 = self.add as i16;

                {
                    let abuf = &mut self.wbuf;
                    abuf.clear();
                    abuf.extend_from_slice(buf);

                    for a in abuf.iter_mut() {
                        *a = (x + *a as i16) as u8;
                    }
                }
                self.write_wbuf(cx)
            }
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        self.base.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        self.base.as_mut().poll_shutdown(cx)
    }
}

// 可生成一个 AdderConn, 其对输入进行加(减)法操作
#[derive(Debug, Clone, Copy, Default)]
pub struct Adder {
    pub addnum: i8,
    pub direction: AddDirection,
}
impl Name for Adder {
    fn name(&self) -> &'static str {
        "adder"
    }
}

impl ToMapper for i8 {
    fn to_mapper(&self) -> MapperBox {
        let mut a = Adder::default();
        a.addnum = *self;
        Box::new(a)
    }
}

#[async_trait]
impl crate::map::Mapper for Adder {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::TCP(c) => {
                let cc = AdderConn {
                    cid,
                    add: self.addnum,
                    base: Box::pin(c),
                    wbuf: BytesMut::with_capacity(1024), //todo change this
                    readl: 0,
                    direction: self.direction,
                };

                MapResult {
                    a: params.a,
                    b: params.b,
                    c: Stream::TCP(Box::new(cc)),
                    d: None,
                    e: None,
                    new_id: None,
                }
            }
            Stream::UDP(_) => {
                todo!()
            }
            Stream::None => MapResult::err_str("adder: can't add without a stream"),
            _ => MapResult::err_str("adder: can't count with a stream generator"),
        }
    }
}
