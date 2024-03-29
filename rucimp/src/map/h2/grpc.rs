pub mod zero_rtt;

use std::{
    cmp::min,
    io::{self, Error, ErrorKind},
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tracing::debug;
const MAX_VARINT_LEN64: usize = 10;

pub const CONTENT_TYPE: &str = "content-type";
pub const GRPC_CONTENT_TYPE: &str = "application/grpc";
pub const USER_AGENT: &str = "rucimp";

pub fn build_grpc_request_from(c: &CommonConfig) -> Request<()> {
    let mut request = Request::builder()
        .method(c.method.as_deref().unwrap_or("POST"))
        .header(CONTENT_TYPE, GRPC_CONTENT_TYPE)
        .header("user-agent", USER_AGENT);

    if c.authority.is_empty() {
        request = request.uri(&c.path)
    } else {
        request = request.uri(
            Uri::builder()
                .scheme(c.scheme.as_deref().unwrap_or("https"))
                .authority(c.authority.as_str())
                .path_and_query(&c.path)
                .build()
                .expect("uri ok"),
        )
    }

    if let Some(h) = &c.headers {
        for (k, v) in h.iter() {
            if k != "Host" {
                request = request.header(k.as_str(), v.as_str());
            }
        }
    }
    request = request.version(Version::HTTP_2);
    request.body(()).unwrap()
}

pub fn match_grpc_request_header<'a, T: 'a>(r: &'a Request<T>) -> Result<(), HttpMatchError<'a>> {
    let r = r
        .headers()
        .get(CONTENT_TYPE)
        .ok_or(HttpMatchError::InvalidContentType {
            expected: GRPC_CONTENT_TYPE,
            found: "",
        })?
        .to_str()
        .expect("ok");
    if r != GRPC_CONTENT_TYPE {
        return Err(HttpMatchError::InvalidContentType {
            expected: GRPC_CONTENT_TYPE,
            found: r,
        });
    }

    Ok(())
}

fn put_uvarint(buf: &mut [u8], mut x: usize) -> usize {
    let mut i = 0;
    while x >= 0x80 {
        buf[i] = (x as u8) | 0x80;
        x >>= 7;
        i += 1;
    }
    buf[i] = x as u8;
    i + 1
}

use crate::net::http::HttpMatchError;
use futures::ready;
use h2::{RecvStream, SendStream};
use http::{Request, Uri, Version};
use ruci::{net::http::CommonConfig, utils::io_error};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::oneshot::Sender,
};

#[derive(Error, Debug)]
pub enum UVariantErr {
    #[error("no first byte")]
    NoFB,

    #[error("overflow")]
    OverFlow,
}

pub fn read_uvarint(r: &mut BytesMut) -> (u64, Option<UVariantErr>) {
    let mut x = 0u64;
    let mut s = 0u8;
    for i in 0..MAX_VARINT_LEN64 {
        if r.is_empty() {
            return (x, Some(UVariantErr::NoFB));
        }
        let b = r.get_u8();

        if b < 0x80 {
            if i == MAX_VARINT_LEN64 - 1 && b > 1 {
                return (x, Some(UVariantErr::OverFlow));
            }
            return (x | (b as u64) << s, None);
        }
        x |= ((b & 0x7f) as u64) << s;
        s += 7;
    }

    (x, Some(UVariantErr::OverFlow))
}

pub fn get_real_len(lp: usize) -> usize {
    let mut protobuf_header = [0u8; MAX_VARINT_LEN64 + 1];
    protobuf_header[0] = 0x0a;

    let varuint_size = put_uvarint(&mut protobuf_header[1..], lp);

    let ph_len = varuint_size + 1;

    5 + ph_len + lp
}

pub fn encode_head(lp: usize, only_head_cap: bool) -> BytesMut {
    let mut protobuf_header = [0u8; MAX_VARINT_LEN64 + 1];
    protobuf_header[0] = 0x0a;

    let varuint_size = put_uvarint(&mut protobuf_header[1..], lp);

    let ph_len = varuint_size + 1;

    let grpc_payload_len: u32 = (ph_len + lp) as u32;

    let mut buf = BytesMut::with_capacity(5 + ph_len + if only_head_cap { 0 } else { lp });
    buf.put_u8(0);
    buf.put_u32(grpc_payload_len);

    buf.extend_from_slice(&protobuf_header[..ph_len]);
    buf
}

pub fn encode(payload: &[u8], only_head: bool) -> BytesMut {
    let mut buf = encode_head(payload.len(), only_head);
    if !only_head {
        buf.extend_from_slice(payload);
    }
    buf
}

const READ_HEAD_LEN: usize = 6;

pub struct Stream {
    recv: RecvStream,
    send: SendStream<Bytes>,
    r_buffer: BytesMut,

    next_data_len: usize,

    shutdown_tx: Option<Sender<()>>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            debug!("grpc got dropped, sending shutdown_tx ");
            let _ = tx.send(());
        }
    }
}

impl Stream {
    #[inline]
    pub fn new(recv: RecvStream, send: SendStream<Bytes>, shutdown_tx: Option<Sender<()>>) -> Self {
        //debug!("new grpc stream");
        Stream {
            recv,
            send,
            r_buffer: BytesMut::with_capacity(super::BUFFER_CAP),
            next_data_len: 0,
            shutdown_tx,
        }
    }

    fn try_read_next_len(&mut self) -> Result<(), UVariantErr> {
        if self.next_data_len == 0 {
            self.r_buffer.advance(READ_HEAD_LEN);
            let (len, oe) = read_uvarint(&mut self.r_buffer);
            if let Some(e) = oe {
                return Err(e);
            }
            self.next_data_len = len as usize;
        }
        Ok(())
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        //debug!("poll_read called");
        loop {
            //debug!("loop {} {}", self.next_data_len, self.buffer.len());

            if !self.r_buffer.is_empty() && self.r_buffer.len() >= self.next_data_len {
                let r = self.try_read_next_len();
                if let Err(e) = r {
                    return Poll::Ready(Err(io_error(e)));
                }

                let v = vec![buf.remaining(), self.r_buffer.len(), self.next_data_len];

                let r_len = v.into_iter().min().expect("ok");

                // debug!(
                //     "is {} {} {} {}",
                //     buf.remaining(),
                //     self.buffer.len(),
                //     self.next_data_len,
                //     r_len
                // );

                let read_data = self.r_buffer.split_to(r_len);
                buf.put_slice(&read_data);

                self.next_data_len -= r_len;

                return Poll::Ready(Ok(()));
            };
            match ready!(self.recv.poll_data(cx)) {
                Some(Ok(data)) => {
                    self.r_buffer.extend_from_slice(&data);

                    let r = self
                        .recv
                        .flow_control()
                        .release_capacity(data.len())
                        .map_err(|e| Error::new(ErrorKind::ConnectionReset, e));
                    if let Err(e) = r {
                        return Poll::Ready(Err(e));
                    }
                }
                // no more data frames
                // maybe trailer
                // or cancelled
                _ => return Poll::Ready(Ok(())),
            }
        }
    }
}

impl AsyncWrite for Stream {
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let old_len = buf.len();
        let realbuf_len = get_real_len(old_len);

        self.send.reserve_capacity(realbuf_len);
        Poll::Ready(match ready!(self.send.poll_capacity(cx)) {
            Some(Ok(write_len)) => {
                if write_len >= realbuf_len {
                    //实测不能连续使用两次 send_data, 会卡住, 故只能先encode再一次性发送

                    let b = encode(buf, false);

                    self.send.send_data(b.into(), false).map_or_else(
                        |e| Err(Error::new(ErrorKind::BrokenPipe, e)),
                        |_| Ok(min(old_len, write_len)),
                    )
                } else {
                    //debug!("write len short {write_len} {realbuf_len}");
                    //这种情况很常见, 尤其在看4k视频等大流量情况下,在 e的测试中, realbuf_len 常为 8200,
                    // 而 实得的 write_len 会以各种数值 小于它

                    let diff = realbuf_len - write_len;
                    let old_buf_written = old_len - diff;

                    let to_write_buf = encode(&buf[..old_buf_written], false);

                    self.send.send_data(to_write_buf.into(), false).map_or_else(
                        |e| Err(Error::new(ErrorKind::BrokenPipe, e)),
                        |_| Ok(old_buf_written),
                    )
                }
            }
            // is_send_streaming returns false
            // which indicates the state is
            // neither open nor half_close_remote
            _ => Err(Error::new(ErrorKind::BrokenPipe, "broken pipe")),
        })
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.send.reserve_capacity(0);
        Poll::Ready(ready!(self.send.poll_capacity(cx)).map_or(
            Err(Error::new(ErrorKind::BrokenPipe, "broken pipe")),
            |_| {
                let r = self
                    .send
                    .send_data(Bytes::new(), true)
                    .map_err(|e| Error::new(ErrorKind::BrokenPipe, e));

                if let Some(tx) = self.shutdown_tx.take() {
                    debug!("grpc sending shutdown_tx ");
                    let _ = tx.send(());
                }
                r
            },
        ))
    }
}
