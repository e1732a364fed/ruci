/*!
Count the traffic bytes of the base connection

* 使用 [`Counter`] 与 [`Arc<GlobalTrafficRecorder>`] 的区别是, [`Arc<GlobalTrafficRecorder>`] 是全局解密流量的统计,
而 [`Counter`] 是针对自己持有的 Conn的流量的统计
*/

use super::*;
use std::{
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::Poll,
};

use crate::map;
use crate::{net::*, Name};
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

use macro_mapper::{mapper_ext_fields, MapperExt};
use tracing::debug;
/// takes ownership of base Conn, for counting
pub struct CounterConn {
    pub data: CounterData,
    base: Pin<net::Conn>,
}

impl Name for CounterConn {
    fn name(&self) -> &str {
        "counter"
    }
}

#[derive(Clone)]
pub struct CounterData {
    pub cid: CID,

    pub ub: Arc<AtomicU64>,
    pub db: Arc<AtomicU64>,
}

impl AsyncRead for CounterConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let previous_len = buf.filled().len();
        let r = self.base.as_mut().poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &r {
            let n = buf.filled().len() - previous_len;

            let db = self.data.db.fetch_add(n as u64, Ordering::Relaxed);
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("{}, counter : db: {}, ", self.data.cid, db,);
            }
        }
        r
    }
}

impl AsyncWrite for CounterConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let r = self.base.as_mut().poll_write(cx, buf);

        if let Poll::Ready(Ok(u)) = &r {
            let ub = self.data.ub.fetch_add(*u as u64, Ordering::Relaxed);
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("{}, counter : ub: {}, ", self.data.cid, ub,);
            }
        }
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

/// 通过 maps 给 base 添加 上传和下载的流量计数
#[mapper_ext_fields]
#[derive(Debug, Clone, Default, MapperExt)]
pub struct Counter {}

impl Name for Counter {
    fn name(&self) -> &'static str {
        "counter"
    }
}

#[async_trait]
impl Mapper for Counter {
    /// ignores behavior
    ///
    /// returns dynamic_data with upload and download
    /// [`Arc<Atomic64>`]`
    ///
    ///
    async fn maps(&self, cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::Conn(c) => {
                let mut db = 0;

                if behavior == ProxyBehavior::DECODE {
                    if let Some(ed) = params.b.as_ref() {
                        db = ed.len() as u64;
                    }
                };

                let cd = CounterData {
                    cid,
                    ub: Arc::new(AtomicU64::new(0)),
                    db: Arc::new(AtomicU64::new(db)),
                };

                let cc = CounterConn {
                    data: cd.clone(),
                    base: Box::pin(c),
                };

                let output_data: Vec<Arc<AtomicU64>> = vec![cd.ub.clone(), cd.db.clone()];

                MapResult::builder()
                    .a(params.a)
                    .b(params.b)
                    .c(Stream::c(Box::new(cc)))
                    .dynamic_data(output_data)
                    .build()
            }
            Stream::AddrConn(_) => {
                todo!()
            }
            Stream::None => MapResult::err_str("counter: can't count without a stream"),
            _ => MapResult::err_str("counter: can't count with a stream generator"),
        }
    }
}
