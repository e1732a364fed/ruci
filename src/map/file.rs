/*

*/
use crate::map;
use async_trait::async_trait;
use log::debug;
use macro_mapper::{mapper_ext_fields, MapperExt};
use std::{pin::Pin, task::Poll};

use crate::{net::CID, Name};

use super::*;
use tokio::{
    fs::File,
    io::{self, AsyncRead, AsyncWrite},
};

#[derive(Debug)]
pub struct FileIOConn {
    pub i: Pin<Box<tokio::fs::File>>,
    pub o: Pin<Box<tokio::fs::File>>,
}

impl Name for FileIOConn {
    fn name(&self) -> &'static str {
        "fileio_conn"
    }
}

impl AsyncRead for FileIOConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let r = self.i.as_mut().poll_read(cx, buf);
        r
    }
}

impl AsyncWrite for FileIOConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let r = self.o.as_mut().poll_write(cx, buf);

        r
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
        self.o.as_mut().poll_shutdown(cx)
    }
}

/// use an existing file as the stream source
#[mapper_ext_fields]
#[derive(Debug, MapperExt)]
pub struct FileIO {
    pub iname: String,
    pub oname: String,
}

impl Name for FileIO {
    fn name(&self) -> &'static str {
        "fileio"
    }
}
impl FileIO {
    async fn get_conn(&self) -> anyhow::Result<FileIOConn> {
        let i = File::open(&self.iname).await?;
        let o = File::open(&self.oname).await?;
        Ok(FileIOConn {
            i: Box::pin(i),
            o: Box::pin(o),
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

        let c = match self.get_conn().await {
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
            debug!("stdio: has pre_defined_early_data");
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
