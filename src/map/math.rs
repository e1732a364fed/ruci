use crate::net;
use async_trait::async_trait;
use bytes::BytesMut;
use std::{io, pin::Pin, task::Poll};
use tokio::io::{AsyncRead, AsyncWrite};

use super::*;

/// 按字节加法器。
///
/// 本端write 被调用时 进行输入，另一端read  被调用时进行输出,
///
/// 输出为输入按字节+1
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
    pub cid: u32,
    base: Pin<net::Conn>,
}

impl AsyncRead for AdderConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let r = self.base.as_mut().poll_read(cx, buf);
        if let Poll::Ready(Ok(_)) = &r {}
        r
    }
}

impl AsyncWrite for AdderConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut abuf = BytesMut::from(buf);
        for a in abuf.iter_mut() {
            *a = ((self.add as i16) + *a as i16) as u8;
        }

        let r = self.base.as_mut().poll_write(cx, &abuf);

        if let Poll::Ready(Ok(_)) = &r {}
        r
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
pub struct Adder {
    pub addnum: i8,
}

#[async_trait]
impl crate::map::Mapper for Adder {
    fn name(&self) -> &'static str {
        "adder"
    }

    async fn maps(
        &self,
        cid: u32, //state 的 id
        _behavior: ProxyBehavior,
        params: MapParams,
    ) -> MapResult {
        match params.c {
            Stream::TCP(c) => {
                let cc = AdderConn {
                    cid,
                    add: self.addnum,
                    base: Box::pin(c),
                };

                MapResult {
                    a: params.a,
                    b: params.b,
                    c: Some(Box::new(cc)),
                    d: None,
                    e: None,
                }
            }
            Stream::UDP(_) => {
                unimplemented!()
            }
            Stream::None => MapResult::err_str("adder: can't add without a stream"),
        }
    }
}
