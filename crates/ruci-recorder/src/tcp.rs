use std::{
    io,
    pin::Pin,
    task::{ready, Poll},
};

use futures::Future;
use pin_project::pin_project;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{debug, info};

use super::Recorder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum State {
    #[default]
    Normal,
    SavingToFile,
    Saved,
}

#[pin_project]
pub(super) struct RecorderConn {
    #[pin]
    pub(super) base: Pin<ruci::net::Conn>,
    pub(super) record: Recorder,
    pub(super) save_future:
        Option<Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + Sync>>>,

    pub(super) state: State,
}

impl RecorderConn {
    /// if save_future is ready, it will set state to Saved
    fn poll_save_future(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.project();
        if let Some(fut) = this.save_future.as_mut() {
            let result = fut.as_mut().poll(cx);
            if result.is_ready() {
                *this.save_future = None;
                *this.state = State::Saved;
            }
            result
        } else {
            Poll::Ready(Ok(()))
        }
    }

    /// must called in normal state
    ///
    /// will set state to SavingToFile
    fn start_save(self: Pin<&mut Self>) {
        let this = self.project();
        if *this.state != State::Normal {
            return;
        }
        *this.state = State::SavingToFile;
        this.record.record_shutdown();
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
        match self.as_mut().project().state {
            State::Normal => {
                let r = self.as_mut().project().base.poll_read(cx, buf);

                if let Poll::Ready(r) = &r {
                    match r {
                        Ok(_) => {
                            let l = buf.filled().len();

                            self.record.record_read(buf.filled()); // 为0时也要记录, 因为这是EOF标志

                            if l == 0 {
                                let cid = self.record.common().cid.to_string();
                                info!(
                                    cid = %cid,
                                    "recorder read got EOF(len=0), Saving to file",
                                );
                                self.as_mut().start_save();
                            }

                            // 一般来说，h2 中的 最后一个client 发送的包 很可能是 17字节，它其实是 h2 的 goaway 帧,
                            // 是 9字节的帧头 + 8字节的 payload，见
                            // https://www.rfc-editor.org/rfc/rfc7540.html#section-6.8

                            // if l == 17 {
                            //     debug!("recorder got 17: {}", buf.filled().escape_ascii());
                            // }
                        }
                        Err(e) => {
                            let cid = self.record.common().cid.to_string();
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

            _ => Poll::Ready(Ok(())),
        }
    }
}

impl AsyncWrite for RecorderConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.as_mut().project().state {
            State::Normal => {
                let r = self.as_mut().project().base.poll_write(cx, buf);

                if let Poll::Ready(Ok(u)) = &r {
                    self.record.record_write(&buf[..*u]);
                }
                r
            }
            _ => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::Other,
                "recorder is not in normal state",
            ))),
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        match self.as_mut().project().state {
            State::Normal => self.project().base.poll_flush(cx),
            _ => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            match *self.as_mut().project().state {
                State::Normal => {
                    info!(
                        cid = %self.record.common().cid,
                        label = %self.record.common().label.as_deref().unwrap_or(""),
                        "recorder got shutdown call, Saving to file",
                    );

                    self.as_mut().start_save();

                    continue;
                }
                State::SavingToFile => match ready!(self.as_mut().poll_save_future(cx)) {
                    Ok(_) => {
                        // trace!("recorder save ready");

                        continue;
                    }
                    Err(e) => {
                        debug!("recorder save got error: {:?}", e);
                        continue;
                    }
                },
                State::Saved => return self.project().base.poll_shutdown(cx),
            }
        }
    }
}
