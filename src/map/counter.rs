/*!
Defines a [`Map`] that counts the traffic bytes of the base connection.

使用 [`Counter`] 与 [`Arc<GlobalTrafficRecorder>`] 的区别是, [`Arc<GlobalTrafficRecorder>`] 是全局解密流量的统计,
而 [`Counter`] 是针对自己持有的 Conn的流量的统计.

Counter 使用 原子的动态数据，方便实时查询
*/

use super::*;
use std::{
    fmt::{Display, Formatter},
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

use crate::map;
use crate::net::*;
use addr_conn::{AsyncReadAddr, AsyncWriteAddr};
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

use macro_map::{map_ext_fields, MapExt};
use tracing::debug;
/// takes ownership of base Conn, for counting
pub struct CounterConn {
    pub data: CounterData,
    base: Pin<net::Conn>,
}

impl Display for CounterConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "counter")
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
#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct Counter {}

impl Counter {
    pub fn boxed() -> MapBox {
        Box::<Counter>::default()
    }
}

impl Display for Counter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "counter")
    }
}

#[async_trait]
impl Map for Counter {
    /// returns `dynamic_data` with upload and download bytes
    /// in [`Arc<Atomic64>`]`
    ///
    ///
    async fn maps(&self, cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
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
        let output_data: Vec<Arc<AtomicU64>> = vec![cd.ub.clone(), cd.db.clone()];

        match params.c {
            Stream::Conn(c) => {
                let cc = CounterConn {
                    data: cd.clone(),
                    base: Box::pin(c),
                };

                MapResult::builder()
                    .a(params.a)
                    .b(params.b)
                    .c(Stream::c(Box::new(cc)))
                    .dynamic_data(output_data)
                    .build()
            }
            Stream::AddrConn(ac) => {
                let cc = AddrConn {
                    r: Box::new(CounterAddrConnR {
                        base: Box::pin(ac.r),
                        data: cd.clone(),
                    }),
                    w: Box::new(CounterAddrConnW {
                        base: Box::pin(ac.w),
                        data: cd,
                    }),
                    default_write_to: ac.default_write_to,
                    // cached_name: "record_ac".to_string(),
                };
                MapResult::builder()
                    .a(params.a)
                    .b(params.b)
                    .c(Stream::AddrConn(cc))
                    .dynamic_data(output_data)
                    .build()
            }
            Stream::None => MapResult::from_err_str("counter: can't init without a stream"),
            _ => MapResult::from_err_str("counter: can't init with a stream generator"),
        }
    }
}

struct CounterAddrConnR {
    base: Pin<Box<dyn addr_conn::AddrReadTrait>>,
    pub data: CounterData,
}

impl Display for CounterAddrConnR {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "counter_ac_r")
    }
}

struct CounterAddrConnW {
    base: Pin<Box<dyn addr_conn::AddrWriteTrait>>,
    pub data: CounterData,
}

impl Display for CounterAddrConnW {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "counter_ac_w")
    }
}

impl AsyncReadAddr for CounterAddrConnR {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let r = self.base.as_mut().poll_read_addr(cx, buf);
        if let Poll::Ready(Ok((n, ad))) = r {
            let db = self.data.db.fetch_add(n as u64, Ordering::Relaxed);
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("{}, counter : db: {}, ", self.data.cid, db,);
            }
            Poll::Ready(io::Result::Ok((n, ad)))
        } else {
            r
        }
    }
}

impl AsyncWriteAddr for CounterAddrConnW {
    fn poll_write_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let r = self.base.as_mut().poll_write_addr(cx, buf, addr);

        if let Poll::Ready(Ok(u)) = r {
            let ub = self.data.ub.fetch_add(u as u64, Ordering::Relaxed);
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("{}, counter : ub: {}, ", self.data.cid, ub,);
            }
            Poll::Ready(io::Result::Ok(u))
        } else {
            r
        }
    }

    fn poll_flush_addr(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.base.as_mut().poll_flush_addr(cx)
    }

    fn poll_close_addr(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.base.as_mut().poll_close_addr(cx)
    }
}
