/*
some math related Mapper s
*/

use crate::map;
use crate::{
    net::{self, Stream, CID},
    Name,
};
use async_trait::async_trait;
use bytes::BytesMut;
use macro_mapper::NoMapperExt;
use std::{io, pin::Pin, task::Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::*;

/// 按字节加法器。
///
/// 输出为输入按字节+(add), 可设置+的方向 AddDirection
///
/// 例如: add 为1, direction为Read 时, 若read 到的值是 `[1,2,3]`,
/// 则将向外输出 `[2,3,4]`
///
pub struct AdderConn {
    pub add: i8,
    pub cid: CID,
    pub direction: AddDirection,

    base: Pin<net::Conn>,
    w_buf: BytesMut,
    r_buf: BytesMut,
}

//todo: 考虑使用 simd 或 rayon; 可以在rucimp包中实现

#[derive(Clone, Copy, Debug, Default)]
pub enum AddDirection {
    #[default]
    Read,
    Write,
    Both,
}

impl AdderConn {
    //write self.w_buf to  self.base
    fn write_by_w_buf(&mut self, cx: &mut std::task::Context<'_>) -> Poll<io::Result<usize>> {
        self.base.as_mut().poll_write(cx, &self.w_buf)
    }

    //read self.base + add to self.w_buf
    fn read_to_r_buf(&mut self, cx: &mut std::task::Context<'_>) -> Poll<io::Result<()>> {
        let a_buf = &mut self.r_buf;
        a_buf.resize(a_buf.capacity(), 0);
        let mut rb = ReadBuf::new(a_buf);

        let r = self.base.as_mut().poll_read(cx, &mut rb);

        let x = rb.filled().len();
        self.r_buf.resize(x, 0);

        let x: i16 = self.add as i16;
        for a in self.r_buf.iter_mut() {
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

impl AsyncRead for AdderConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.direction {
            AddDirection::Write => self.base.as_mut().poll_read(cx, buf),
            _ => {
                let r = self.read_to_r_buf(cx);

                if let Poll::Ready(Ok(_)) = &r {
                    buf.put_slice(&self.r_buf);
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
                    let a_buf = &mut self.w_buf;
                    a_buf.clear();
                    a_buf.extend_from_slice(buf);

                    for a in a_buf.iter_mut() {
                        *a = (x + *a as i16) as u8;
                    }
                }
                self.write_by_w_buf(cx)
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

/// maps generates an AdderConn stream, which does add or sub for the input
#[derive(Debug, Clone, Copy, Default, NoMapperExt)]
pub struct Adder {
    pub add_num: i8,
    pub direction: AddDirection,
}
impl Name for Adder {
    fn name(&self) -> &'static str {
        "adder"
    }
}
impl std::fmt::Display for Adder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "adder {:?} {}", self.direction, self.add_num)
    }
}

impl ToMapperBox for i8 {
    /// AddDirection = Read
    fn to_mapper_box(&self) -> MapperBox {
        Box::new(Adder {
            add_num: *self,
            ..Default::default()
        })
    }
}

#[async_trait]
impl crate::map::Mapper for Adder {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::Conn(c) => {
                let cc = AdderConn {
                    cid,
                    add: self.add_num,
                    base: Box::pin(c),
                    w_buf: BytesMut::with_capacity(1024), //todo change this
                    r_buf: BytesMut::with_capacity(1024), //todo change this
                    direction: self.direction,
                };

                MapResult::new_c(Box::new(cc))
                    .a(params.a)
                    .b(params.b)
                    .build()
            }
            Stream::AddrConn(_) => {
                todo!()
            }
            Stream::None => MapResult::err_str("adder: can't add without a stream"),
            _ => MapResult::err_str("adder: can't count with a stream generator"),
        }
    }
}
