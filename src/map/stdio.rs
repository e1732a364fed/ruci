/*!
* write, read 到 stdio (标准输入输出，即命令行). 理解上, stdio 和 tcp_dialer 类似,
  都是 一种 【 单流发生器 】
*/

use std::{pin::Pin, task::Poll};

use async_trait::async_trait;

use crate::{net::CID, Name};

use super::*;
use tokio::io::{self, AsyncRead, AsyncWrite, Stdin, Stdout};

pub struct Conn {
    input: Pin<Box<Stdin>>,
    out: Pin<Box<Stdout>>,
}
impl Name for Conn {
    fn name(&self) -> &'static str {
        "stdio_conn"
    }
}

impl AsyncRead for Conn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let r = self.input.as_mut().poll_read(cx, buf);
        r
    }
}

impl AsyncWrite for Conn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let r = self.out.as_mut().poll_write(cx, buf);

        r
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        self.out.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        self.out.as_mut().poll_shutdown(cx)
    }
}

#[derive(Clone, Debug)]
pub struct Stdio {
    // 存一个可选的addr, 可作为指定的连接目标。 这样stdio的decode行为就像一个 socks5一样
    pub addr: Option<net::Addr>,
}

impl Name for Stdio {
    fn name(&self) -> &'static str {
        "stdio"
    }
}

impl Stdio {
    pub fn from(s: &str) -> MapperBox {
        if s.is_empty() {
            Box::new(Stdio { addr: None })
        } else {
            let a = net::Addr::from_network_addr_str(s).unwrap();
            Box::new(Stdio { addr: Some(a) })
        }
    }
}

#[async_trait]
impl Mapper for Stdio {
    fn configured_target_addr(&self) -> Option<net::Addr> {
        self.addr.clone()
    }

    async fn maps(&self, _cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        if params.c.is_not_none() {
            return MapResult::err_str("stdio can't generate stream when there's already one");
        };

        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let c = Conn {
            input: Box::pin(stdin),
            out: Box::pin(stdout),
        };

        let a = if params.a.is_some() {
            params.a
        } else {
            self.configured_target_addr()
        };
        MapResult::oabc(a, params.b, Box::new(c))
    }
}
