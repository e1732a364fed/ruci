use std::{io, pin::Pin, task::Poll};

use futures::Future;
use pin_project::pin_project;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::info;

use super::Recorder;

#[pin_project]
pub(super) struct RecorderConn {
    #[pin]
    pub(super) base: Pin<ruci::net::Conn>,
    pub(super) record: Recorder,
    pub(super) save_future: Option<Pin<Box<dyn Future<Output = ()> + Send + Sync>>>,
}

impl RecorderConn {
    fn poll_save_future(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<()> {
        let this = self.project();
        if let Some(fut) = this.save_future.as_mut() {
            let res = fut.as_mut().poll(cx);
            if res.is_ready() {
                *this.save_future = None;
            }
            res
        } else {
            Poll::Ready(())
        }
    }

    fn start_save(self: Pin<&mut Self>) {
        let this = self.project();
        if this.save_future.is_none() {
            *this.save_future = Some(Box::pin(this.record.async_save()));
        }
    }
}

impl AsyncRead for RecorderConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // First poll the save future if it exists
        let _ = self.as_mut().poll_save_future(cx);

        let r = self.as_mut().project().base.poll_read(cx, buf);

        if let Poll::Ready(r) = &r {
            match r {
                Ok(_) => {
                    let l = buf.filled().len();
                    if l == 0 {
                        let cid = self.record.cid().to_string();
                        info!(
                            cid = %cid,
                            "recorder read got EOF(len=0), Saving to file",
                        );
                        self.as_mut().start_save();
                    } else {
                        self.record.record_d(buf.filled())
                    }
                }
                Err(e) => {
                    let cid = self.record.cid().to_string();
                    info!(
                        cid = %cid,
                        "recorder read got err, Saving to file; err: {e}",
                    );
                    self.as_mut().start_save();
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
        // First poll the save future if it exists
        let _ = self.as_mut().poll_save_future(cx);

        let r = self.as_mut().project().base.poll_write(cx, buf);

        if let Poll::Ready(Ok(u)) = &r {
            self.record.record_u(&buf[..*u]);
        }
        r
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        // First poll the save future if it exists
        let _ = self.as_mut().poll_save_future(cx);

        self.project().base.poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        // First poll the save future if it exists
        let _ = self.as_mut().poll_save_future(cx);

        let cid = self.record.cid().to_string();
        let label = self.record.label().to_string();

        info!(
            cid = %cid,
            label = %label,
            "recorder got shutdown, Saving to file...",
        );

        self.as_mut().start_save();

        // Poll again to make progress on the save
        let _ = self.as_mut().poll_save_future(cx);

        self.project().base.poll_shutdown(cx)
    }
}
