use std::{
    cmp::min,
    io::{self, Error, ErrorKind},
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
const MAX_VARINT_LEN64: usize = 10;

pub const CONTENT_TYPE: &str = "content-type";
pub const GRPC_CONTENT_TYPE: &str = "application/grpc";
pub const USER_AGENT: &str = "rucimp";

pub fn build_grpc_request_from(c: &CommonConfig) -> Request<()> {
    let mut request = Request::builder()
        .method(c.method.as_deref().unwrap_or("POST"))
        .header(CONTENT_TYPE, GRPC_CONTENT_TYPE)
        .header("user-agent", USER_AGENT)
        .header("Te", "trailers");

    if c.host.is_empty() {
        request = request.uri(&c.path)
    } else {
        request = request.header("Host", c.host.as_str()).uri(
            Uri::builder()
                .scheme(c.scheme.as_deref().unwrap_or("https"))
                .authority(c.host.as_str())
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

use crate::net::HttpMatchError;
use futures::ready;
use h2::{RecvStream, SendStream};
use http::{Request, Uri};
use ruci::{net::http::CommonConfig, utils::io_error};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};

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

pub fn encode(payload: &[u8]) -> BytesMut {
    let mut protobuf_header = [0u8; MAX_VARINT_LEN64 + 1];
    protobuf_header[0] = 0x0a;

    let lp = payload.len();
    let varuint_size = put_uvarint(&mut protobuf_header[1..], lp);

    let ph_len = varuint_size + 1;

    let grpc_payload_len: u32 = (ph_len + lp) as u32;

    let mut buf = BytesMut::with_capacity(5 + ph_len + payload.len());
    buf.put_u8(0);
    buf.put_u32(grpc_payload_len);

    buf.extend_from_slice(&protobuf_header[..ph_len]);
    buf.extend_from_slice(payload);
    buf
}

const READ_HEAD_LEN: usize = 6;

pub struct Stream {
    recv: RecvStream,
    send: SendStream<Bytes>,
    buffer: BytesMut,

    next_data_len: usize,
}

impl Stream {
    #[inline]
    pub fn new(recv: RecvStream, send: SendStream<Bytes>) -> Self {
        //debug!("new grpc stream");
        Stream {
            recv,
            send,
            buffer: BytesMut::with_capacity(super::BUFFER_CAP),
            next_data_len: 0,
        }
    }

    fn try_read_next_len(&mut self) -> Result<(), UVariantErr> {
        if self.next_data_len == 0 {
            self.buffer.advance(READ_HEAD_LEN);
            let (len, oe) = read_uvarint(&mut self.buffer);
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

            if !self.buffer.is_empty() && self.buffer.len() >= self.next_data_len {
                let r = self.try_read_next_len();
                if let Err(e) = r {
                    return Poll::Ready(Err(io_error(e)));
                }

                let v = vec![buf.remaining(), self.buffer.len(), self.next_data_len];

                let r_len = v.into_iter().min().expect("ok");

                // debug!(
                //     "is {} {} {} {}",
                //     buf.remaining(),
                //     self.buffer.len(),
                //     self.next_data_len,
                //     r_len
                // );

                let read_data = self.buffer.split_to(r_len);
                buf.put_slice(&read_data);

                self.next_data_len -= r_len;

                return Poll::Ready(Ok(()));
            };
            match ready!(self.recv.poll_data(cx)) {
                Some(Ok(data)) => {
                    self.buffer.extend_from_slice(&data);

                    let r = self
                        .recv
                        .flow_control()
                        .release_capacity(data.len())
                        .map_or_else(
                            |e| Err(Error::new(ErrorKind::ConnectionReset, e)),
                            |_| Ok(()),
                        );
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
        let real_buf = encode(buf);

        self.send.reserve_capacity(real_buf.len());
        Poll::Ready(match ready!(self.send.poll_capacity(cx)) {
            Some(Ok(write_len)) => self.send.send_data(real_buf.into(), false).map_or_else(
                |e| Err(Error::new(ErrorKind::BrokenPipe, e)),
                |_| Ok(min(old_len, write_len)),
            ),
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
                self.send
                    .send_data(Bytes::new(), true)
                    .map_or_else(|e| Err(Error::new(ErrorKind::BrokenPipe, e)), |_| Ok(()))
            },
        ))
    }
}
