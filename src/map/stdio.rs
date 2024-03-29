/*!
Defines a Map that write, read stdio (标准输入输出, 即命令行).

在流行为上, stdio 和 [`crate::map::network::BindDialer`] 类似,  都是 一种 【 单流发生器 】
*/

use crate::map;
use async_trait::async_trait;
use macro_map::{map_ext_fields, MapExt};
use std::{pin::Pin, task::Poll};
use tracing::debug;

use crate::{net::CID, Name};

use super::*;
use tokio::io::{self, AsyncRead, AsyncWrite, AsyncWriteExt, Stdin, Stdout};

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
        // 不能向windows 的 stdio 输出 非 utf8 信息, or we will get
        // Windows stdio in console mode does not support writing non-UUTF-8 byte sequence

        let old_len = buf.len();
        let str = String::from_utf8_lossy(buf);
        let sb = str.as_bytes();
        let r = self.out.as_mut().poll_write(cx, sb);
        match r {
            Poll::Ready(r) => match r {
                Ok(u) => {
                    if sb.len() == u {
                        Poll::Ready(Ok(old_len))
                    } else {
                        Poll::Ready(Ok(old_len - sb.len() + u))
                    }
                }
                Err(e) => Poll::Ready(Err(e)),
            },
            Poll::Pending => Poll::Pending,
        }
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

#[map_ext_fields]
#[derive(Clone, Debug, Default, MapExt)]
pub struct Stdio {}

impl Name for Stdio {
    fn name(&self) -> &'static str {
        "stdio"
    }
}

impl Stdio {
    pub fn boxed() -> MapBox {
        Box::<Stdio>::default()
    }
}

#[async_trait]
impl Map for Stdio {
    async fn maps(&self, _cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        if params.c.is_some() {
            return MapResult::err_str("stdio can't generate stream when there's already one");
        };

        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let mut c = Conn {
            input: Box::pin(stdin),
            out: Box::pin(stdout),
        };

        let a = if params.a.is_some() {
            params.a
        } else {
            self.configured_target_addr().cloned()
        };

        let mut buf = params.b;
        if let Some(ped) = self.get_pre_defined_early_data() {
            debug!("stdio: has pre_defined_early_data");
            match buf {
                Some(mut bf) => {
                    bf.extend_from_slice(&ped);
                    buf = Some(bf)
                }
                None => buf = Some(ped),
            }
        }
        if let Some(ed) = buf.as_ref() {
            if self.is_tail_of_chain() {
                debug!("stdio: write earlydata");
                let r = c.write(ed).await;
                if let Err(e) = r {
                    return MapResult::from_e(e);
                }
                let r = c.flush().await; //this flush is necessary
                if let Err(e) = r {
                    return MapResult::from_e(e);
                }

                buf = None;
            }
        }
        MapResult::new_c(Box::new(c)).b(buf).a(a).build()
    }
}
