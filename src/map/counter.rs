/*!
 * 使用 Counter 与 Arc<TransmissionInfo> 的区别是, Arc<TransmissionInfo> 是全局解密流量的统计，
 * 而Counter是针对自己持有的 Conn的流量的统计
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

use crate::{net::*, Name};
use async_trait::async_trait;
use log::{debug, log_enabled};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

/// 持有上层Conn的所有权，用于计数
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
            if log_enabled!(log::Level::Debug) {
                debug!(
                    "cid: {}, counter for {}: db: {}, ",
                    self.data.cid,
                    self.base.name(),
                    db,
                );
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
            if log_enabled!(log::Level::Debug) {
                debug!(
                    "cid: {}, counter for {}: ub: {}, ",
                    self.data.cid,
                    self.base.name(),
                    ub,
                );
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
#[derive(Debug, Clone)]
pub struct Counter;

impl Name for Counter {
    fn name(&self) -> &'static str {
        "counter"
    }
}

#[async_trait]
impl Mapper for Counter {
    /// 生成的 MapResult 中的 d 为  Box<CounterData>
    ///
    /// 不分 behavior
    async fn maps(
        &self,
        cid: CID, //state 的 id
        _behavior: ProxyBehavior,
        params: MapParams,
    ) -> MapResult {
        match params.c {
            Stream::TCP(mut c) => {
                let mut ub = 0;
                if let Some(ed) = params.b {
                    let r = c.write(&ed).await;
                    match r {
                        Ok(r) => ub = r as u64,
                        Err(e) => {
                            return MapResult::from_err(e);
                        }
                    }
                }

                let cd = CounterData {
                    cid,
                    ub: Arc::new(AtomicU64::new(ub)),
                    db: Arc::new(AtomicU64::new(0)),
                };

                let cc = CounterConn {
                    data: cd.clone(),
                    base: Box::pin(c),
                };

                MapResult {
                    a: params.a,
                    b: None,
                    c: Stream::TCP(Box::new(cc)),
                    d: Some(AnyData::B(Box::new(cd))),
                    e: None,
                }
            }
            Stream::UDP(_) => {
                unimplemented!()
            }
            Stream::None => MapResult::err_str("counter: can't count without a stream"),
            _ => MapResult::err_str("counter: can't count with a stream generator"),
        }
    }
}
