/*!
Defines a [`Map`] that records the traffic bytes of the base connection.

format: [`RecordData`]， [`RecordAddrData`]
*/

use super::*;
use std::task::Context;
use std::time;
use std::{io, pin::Pin, task::Poll};

use crate::map;
use crate::{net::*, Name};
use addr_conn::{AsyncReadAddr, AsyncWriteAddr};
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

use macro_map::{map_ext_fields, MapExt};

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct RecordData {
    pub cid: CID,
    pub behavior: ProxyBehavior,
    pub custom_str: String,
    pub upload_data: Vec<DataPiece>,
    pub download_data: Vec<DataPiece>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct DataPiece {
    /// duration since the start of the connection
    pub time: std::time::Duration,

    /// data send/recv at this instant
    pub data: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct RecordAddrData {
    pub cid: CID,
    pub behavior: ProxyBehavior,
    pub custom_str: String,

    pub upload_data: Vec<DataAddrPiece>,
    pub download_data: Vec<DataAddrPiece>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct DataAddrPiece {
    /// duration since the start of the connection
    pub time: std::time::Duration,

    /// data send/recv at this instant with the remote addr
    pub data: (Addr, Vec<u8>),
}

/// takes ownership of base Conn
pub struct RecorderConn {
    base: Pin<net::Conn>,
    start: time::Instant,
    record_buffer: RecordData,
}

impl Name for RecorderConn {
    fn name(&self) -> &str {
        "recorder"
    }
}
impl RecorderConn {
    fn since(&self) -> time::Duration {
        time::Instant::now().duration_since(self.start)
    }
}

impl AsyncRead for RecorderConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let r = self.base.as_mut().poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &r {
            let d = self.since();
            self.record_buffer.download_data.push(DataPiece {
                time: d,
                data: buf.filled().to_vec(),
            })
        }
        r
    }
}

impl AsyncWrite for RecorderConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let r = self.base.as_mut().poll_write(cx, buf);

        if let Poll::Ready(Ok(u)) = &r {
            let d = self.since();
            self.record_buffer.upload_data.push(DataPiece {
                time: d,
                data: buf[..*u].to_vec(),
            })
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

struct RecordAddrConnR {
    base: Pin<Box<dyn addr_conn::AddrReadTrait>>,
    start: time::Instant,
    record_buffer: RecordAddrData,
}
impl RecordAddrConnR {
    fn since(&self) -> time::Duration {
        time::Instant::now().duration_since(self.start)
    }
}

impl crate::Name for RecordAddrConnR {
    fn name(&self) -> &str {
        "recorder_ac_r"
    }
}

struct RecordAddrConnW {
    base: Pin<Box<dyn addr_conn::AddrWriteTrait>>,
    start: time::Instant,
    record_buffer: RecordAddrData,
}
impl RecordAddrConnW {
    fn since(&self) -> time::Duration {
        time::Instant::now().duration_since(self.start)
    }
}

impl crate::Name for RecordAddrConnW {
    fn name(&self) -> &str {
        "recorder_ac_w"
    }
}

impl AsyncReadAddr for RecordAddrConnR {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let r = self.base.as_mut().poll_read_addr(cx, buf);
        if let Poll::Ready(r) = r {
            match r {
                Ok((n, ad)) => {
                    let d = self.since();
                    self.record_buffer.download_data.push(DataAddrPiece {
                        time: d,
                        data: (ad.clone(), buf.to_vec()),
                    });
                    Poll::Ready(io::Result::Ok((n, ad)))
                }
                Err(_) => Poll::Ready(r),
            }
        } else {
            r
        }
    }
}

impl AsyncWriteAddr for RecordAddrConnW {
    fn poll_write_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let r = self.base.as_mut().poll_write_addr(cx, buf, addr);

        if let Poll::Ready(r) = r {
            match r {
                Ok(n) => {
                    let d = self.since();
                    self.record_buffer.upload_data.push(DataAddrPiece {
                        time: d,
                        data: (addr.clone(), buf[..n].to_vec()),
                    });

                    Poll::Ready(io::Result::Ok(n))
                }
                Err(_) => Poll::Ready(r),
            }
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

#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct Recorder {
    pub custom_str: String,
}

impl Name for Recorder {
    fn name(&self) -> &'static str {
        "recorder"
    }
}

#[async_trait]
impl Map for Recorder {
    async fn maps(&self, cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let now = time::Instant::now();
        match params.c {
            Stream::Conn(c) => {
                let cc = RecorderConn {
                    base: Box::pin(c),
                    start: now,
                    record_buffer: RecordData {
                        cid: cid.clone(),
                        behavior,
                        custom_str: self.custom_str.clone(),
                        ..Default::default()
                    },
                };

                MapResult::builder()
                    .a(params.a)
                    .b(params.b)
                    .c(Stream::c(Box::new(cc)))
                    .build()
            }
            Stream::AddrConn(ac) => {
                let cc = AddrConn {
                    r: Box::new(RecordAddrConnR {
                        base: Box::pin(ac.r),
                        start: now,
                        record_buffer: RecordAddrData {
                            cid: cid.clone(),
                            behavior,
                            custom_str: self.custom_str.clone(),

                            ..Default::default()
                        },
                    }),
                    w: Box::new(RecordAddrConnW {
                        base: Box::pin(ac.w),
                        start: now,
                        record_buffer: RecordAddrData {
                            cid: cid.clone(),
                            behavior,
                            custom_str: self.custom_str.clone(),

                            ..Default::default()
                        },
                    }),
                    default_write_to: ac.default_write_to,
                    cached_name: "record_ac".to_string(),
                };

                MapResult::builder()
                    .a(params.a)
                    .b(params.b)
                    .c(Stream::AddrConn(cc))
                    .build()
            }
            Stream::None => MapResult::err_str("recorder: can't init without a stream"),
            _ => MapResult::err_str("recorder: can't init with a stream generator"),
        }
    }
}
