/*

*/
use crate::map;
use async_trait::async_trait;
use log::debug;
use macro_mapper::{mapper_ext_fields, MapperExt};
use std::{cmp::min, pin::Pin, task::Poll, time::Duration};

use crate::{net::CID, Name};

use super::*;
use tokio::{
    fs::File,
    io::{self, AsyncRead, AsyncWrite, ReadBuf},
};

#[derive(Debug)]
pub struct FileIOConn {
    pub i: Pin<Box<tokio::fs::File>>,
    pub o: Pin<Box<tokio::fs::File>>,

    sleep_interval: Option<Duration>,
    bytes_per_turn: Option<usize>,
    last_read: Option<tokio::time::Instant>,
}

impl Name for FileIOConn {
    fn name(&self) -> &'static str {
        "fileio_conn"
    }
}
impl FileIOConn {
    fn real_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.bytes_per_turn {
            Some(b) => {
                let mut bf = BytesMut::zeroed(b);
                let mut rb = ReadBuf::new(&mut bf);

                let r = self.i.as_mut().poll_read(cx, &mut rb);

                let rbf = rb.filled();
                if !rbf.is_empty() {
                    let minl = min(rbf.len(), buf.remaining());

                    buf.put_slice(&rbf[..minl]);
                }
                r
            }
            None => self.i.as_mut().poll_read(cx, buf),
        }
    }
}

impl AsyncRead for FileIOConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.sleep_interval {
            Some(si) => match self.last_read {
                Some(last) if last.elapsed() < si => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                _ => {
                    self.last_read = Some(tokio::time::Instant::now());

                    self.real_read(cx, buf)
                }
            },
            None => self.real_read(cx, buf),
        }
    }
}

impl AsyncWrite for FileIOConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.o.as_mut().poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        self.o.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        let _ = self.o.as_mut().poll_shutdown(cx);
        self.i.as_mut().poll_shutdown(cx)
    }
}

/// use an existing file as the stream source.
///
/// ## Write:
/// append or create the file
#[mapper_ext_fields]
#[derive(Debug, MapperExt)]
pub struct FileIO {
    pub iname: String,
    pub oname: String,

    pub sleep_interval: Option<Duration>,
    pub bytes_per_turn: Option<usize>,
}

impl Name for FileIO {
    fn name(&self) -> &'static str {
        "fileio"
    }
}
impl FileIO {
    async fn get_conn(
        &self,
        sleep_interval: Option<Duration>,
        bytes_per_turn: Option<usize>,
    ) -> anyhow::Result<FileIOConn> {
        let i = File::open(&self.iname).await?;
        let o = File::options().append(true).open(&self.oname).await?;
        Ok(FileIOConn {
            i: Box::pin(i),
            o: Box::pin(o),
            sleep_interval,
            bytes_per_turn,
            last_read: None,
        })
    }
}

#[async_trait]
impl Mapper for FileIO {
    async fn maps(&self, _cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        // function is similar to Stdio

        if params.c.is_some() {
            return MapResult::err_str("fileio can't generate stream when there's already one");
        };

        let c = match self
            .get_conn(self.sleep_interval, self.bytes_per_turn)
            .await
        {
            Ok(f) => f,
            Err(e) => return MapResult::from_e(e.context("FileIO init files failed")),
        };

        let a = if params.a.is_some() {
            params.a
        } else {
            self.configured_target_addr().map(|dr| dr.clone())
        };

        let mut buf = params.b;
        if let Some(ped) = self.get_pre_defined_early_data() {
            debug!("Fileio: has pre_defined_early_data");
            match buf {
                Some(mut bf) => {
                    bf.extend_from_slice(&ped);
                    buf = Some(bf)
                }
                None => buf = Some(ped),
            }
        }

        MapResult::newc(Box::new(c)).b(buf).a(a).build()
    }
}
