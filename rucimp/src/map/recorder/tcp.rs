use std::{io, pin::Pin, task::Poll};

// use ruci::Name;
use tokio::io::{AsyncRead, AsyncWrite};

use tracing::info;

use super::data::Recorder;

/// takes ownership of base Conn
pub(super) struct RecorderConn {
    pub(super) base: Pin<ruci::net::Conn>,
    pub(super) record: Recorder,
}

// impl Name for RecorderConn {
//     fn name(&self) -> &str {
//         "recorder"
//     }
// }

impl AsyncRead for RecorderConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let r = self.base.as_mut().poll_read(cx, buf);

        if let Poll::Ready(r) = &r {
            match r {
                Ok(_) => {
                    let l = buf.filled().len();

                    //tcp EOF 的情况
                    if l == 0 {
                        info!(
                            cid = %self.record.data.cid,
                            "recorder read got EOF(len=0), Saving to file",
                        );

                        self.record.data.save();
                    } else {
                        self.record.record_d(buf.filled())
                    }
                }
                Err(e) => {
                    info!(
                        cid = %self.record.data.cid,
                        "recorder read got err, Saving to file; err: {e}",
                    );

                    self.record.data.save();
                }
            }
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
            self.record.record_u(&buf[..*u]);
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
        info!(
            cid = %self.record.data.cid, label = self.record.data.label,
            "recorder got shutdown, Saving to file...",
        );

        self.record.data.save();
        self.base.as_mut().poll_shutdown(cx)
    }
}
