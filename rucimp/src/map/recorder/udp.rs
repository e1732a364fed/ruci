use super::*;
use std::task::Context;
use std::{io, pin::Pin, task::Poll};

use ruci::net::addr_conn::{AsyncReadAddr, AsyncWriteAddr};
use ruci::net::*;

use tracing::info;

pub(super) struct RecordAddrConnR {
    pub(super) base: Pin<Box<dyn addr_conn::AddrReadTrait>>,
    pub(super) record: Recorder,
}

// impl ruci::Name for RecordAddrConnR {
//     fn name(&self) -> &str {
//         "recorder_ac_r"
//     }
// }

pub(super) struct RecordAddrConnW {
    pub(super) base: Pin<Box<dyn addr_conn::AddrWriteTrait>>,
    pub(super) record: Recorder,
}

// impl ruci::Name for RecordAddrConnW {
//     fn name(&self) -> &str {
//         "recorder_ac_w"
//     }
// }

impl AsyncReadAddr for RecordAddrConnR {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let r = self.base.as_mut().poll_read_addr(cx, buf);
        if let Poll::Ready(Ok((n, ad))) = r {
            self.record.r_d_ad(buf, &ad);

            Poll::Ready(io::Result::Ok((n, ad)))
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

        if let Poll::Ready(Ok(n)) = r {
            self.record.r_u_ad(&buf[..n], addr);

            Poll::Ready(io::Result::Ok(n))
        } else {
            r
        }
    }

    fn poll_flush_addr(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.base.as_mut().poll_flush_addr(cx)
    }

    fn poll_close_addr(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        info!(
            cid = %self.record.data.cid,
            "recorder ac got shutdown, Saving to file..."
        );
        self.record.data.save();

        self.base.as_mut().poll_close_addr(cx)
    }
}
