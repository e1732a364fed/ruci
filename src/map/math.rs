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
/// 输出为输入按字节+(add), 可设置+的方向 AddDirection
///
/// 例如: add 为1, direction为Read 时, 若read 到的值是 [1,2,3],
/// 则将向外输出 [2,3,4]
///
pub struct AdderConn {
    pub add: i8,
    pub cid: CID,
    pub direction: AddDirection,

    base: Pin<net::Conn>,
    wbuf: BytesMut,
    rbuf: BytesMut,
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
    fn write_by_wbuf(&mut self, cx: &mut std::task::Context<'_>) -> Poll<io::Result<usize>> {
        self.base.as_mut().poll_write(cx, &self.wbuf)
    }

    //read self.base + add to self.wbuf
    fn read_to_rbuf(&mut self, cx: &mut std::task::Context<'_>) -> Poll<io::Result<()>> {
        let mut abuf = &mut self.rbuf;
        abuf.resize(abuf.capacity(), 0);
        let mut rb = ReadBuf::new(&mut abuf);

        let r = self.base.as_mut().poll_read(cx, &mut rb);

        let x = rb.filled().len();
        self.rbuf.resize(x, 0);

        let x: i16 = self.add as i16;
        for a in self.rbuf.iter_mut() {
            *a = (x + *a as i16) as u8;
        }

        r
    }
}
impl Name for AdderConn {
    fn name(&self) -> &'static str {
        match self.direction {
            AddDirection::Read => "adder_conn(r)",
            AddDirection::Write => "adder_conn(w)",
            AddDirection::Both => "adder_conn",
        }
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
                let r = self.read_to_rbuf(cx);

                if let Poll::Ready(Ok(_)) = &r {
                    buf.put_slice(&self.rbuf);
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
                self.write_by_wbuf(cx)
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
    /// AddDirection = Read
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
                    rbuf: BytesMut::with_capacity(1024), //todo change this
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
