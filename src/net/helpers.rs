/*!
Defines some address related helper functions, some wrappers that wraps AsyncConn to
provide more features, and some fake Stream implementations for debugging.
 */

use std::{
    io,
    net::Ipv4Addr,
    pin::Pin,
    task::{ready, Poll},
};

use crate::Name;

use super::*;
use bytes::{Buf, BufMut, BytesMut};
use parking_lot::Mutex;
use tokio::{io::ReadBuf, sync::mpsc};

use futures::task::Context;
use tracing::trace;

use std::cmp::min;

///max len is 2 + 2 + 255 (domain)
pub const MAX_LEN_SOCKS5_BYTES: usize = 2 + 2 + 255;

/// Read the buf, advance it and parse out the Addr
///
/// todo: add unit test
pub fn socks5_bytes_to_addr(buf: &mut BytesMut) -> anyhow::Result<Addr> {
    if buf.len() < 7 {
        bail!("socks5_bytes_to_addr length wrong1, {}", buf.len());
    }
    let ipn: IPName;
    let at = buf[0];
    buf.advance(1);
    pub const ATYP_IP4: u8 = 1;
    pub const ATYP_DOMAIN: u8 = 3;
    pub const ATYP_IP6: u8 = 4;
    match at {
        ATYP_IP4 => {
            if buf.len() < 6 {
                bail!("socks5_bytes_to_addr length wrong2, {}", buf.len());
            }
            let num = buf.get_u32();
            ipn = IPName::IP(IpAddr::V4(Ipv4Addr::from(num)));
        }
        ATYP_IP6 => {
            if buf.len() < 18 {
                bail!("socks5_bytes_to_addr length wrong3, {}", buf.len());
            }

            let num = buf.get_u128();
            ipn = IPName::IP(IpAddr::V6(Ipv6Addr::from(num)));
        }
        ATYP_DOMAIN => {
            if buf.len() < 4 {
                bail!("socks5_bytes_to_addr length wrong4, {}", buf.len());
            }

            let dn = buf[0] as usize;
            buf.advance(1);

            if buf.len() < dn + 2 {
                bail!("socks5_bytes_to_addr length wrong5, {}", buf.len());
            }
            ipn = IPName::Name(String::from_utf8_lossy(&buf[..dn]).to_string());
            buf.advance(dn);
        }
        _ => bail!("socks5_bytes_to_addr atyp wrong, {}", at),
    }

    Ok(Addr::from_ipname(ipn, buf.get_u16()))
}

pub fn so_to_socks5_bytes(so: SocketAddr, buf: &mut BytesMut) {
    pub const ATYP_IP4: u8 = 1;
    pub const ATYP_IP6: u8 = 4;
    match so.ip() {
        IpAddr::V4(v4) => {
            buf.put_u8(ATYP_IP4);
            buf.extend_from_slice(&v4.octets());
            buf.put_u16(so.port());
        }
        IpAddr::V6(v6) => {
            buf.put_u8(ATYP_IP6);
            buf.extend_from_slice(&v6.octets());
            buf.put_u16(so.port());
        }
    }
}

pub fn addr_to_socks5_bytes(ta: &Addr, buf: &mut BytesMut) {
    pub const ATYP_DOMAIN: u8 = 3;
    match &ta.addr {
        NetAddr::Socket(so) => so_to_socks5_bytes(*so, buf),
        NetAddr::Name(n, p) => {
            buf.put_u8(ATYP_DOMAIN);
            let nbs = n.as_bytes();
            buf.put_u8(nbs.len() as u8);
            buf.extend_from_slice(nbs);
            buf.put_u16(*p);
        }
        NetAddr::NameAndSocket(n, so, p) => {
            let nbs = n.as_bytes();

            if nbs.len() > 255 {
                so_to_socks5_bytes(*so, buf)
            } else {
                buf.put_u8(ATYP_DOMAIN);
                buf.put_u8(nbs.len() as u8);
                buf.extend_from_slice(nbs);
                buf.put_u16(*p);
            }
        }
    }
}

/// wrap [`mpsc::Receiver<BytesMut>`] as a readonly AsyncConn
///
/// Its write method will return OK(n) immediately with input buf, buf.len() == n
pub struct MpscRWrapper {
    pub r: mpsc::Receiver<BytesMut>,
}

impl AsyncRead for MpscRWrapper {
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let r = Pin::new(&mut self.r).poll_recv(cx);
        match ready!(r) {
            Some(mut b) => {
                b.truncate(buf.capacity());
                buf.put(b);
                Poll::Ready(Ok(()))
            }
            None => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "MpscRWrapper r got none",
            ))),
        }
    }
}

impl AsyncWrite for MpscRWrapper {
    #[inline]
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// Wrap R,W as an AsyncConn
pub struct RWWrapper<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> {
    pub r: R,
    pub w: W,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> RWWrapper<R, W> {
    pub fn split(self) -> (R, W) {
        (self.r, self.w)
    }
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> AsyncRead for RWWrapper<R, W> {
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.r).poll_read(cx, buf)
    }
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> AsyncWrite for RWWrapper<R, W> {
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.w).poll_write(cx, buf)
    }

    #[inline]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.w).poll_flush(cx)
    }

    #[inline]
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.w).poll_shutdown(cx)
    }
}

/// wrap base connection with an early data buffer, read from
/// the buffer first.
pub struct EarlyDataWrapper {
    ed: Option<BytesMut>,
    base: Pin<Conn>,
}

impl EarlyDataWrapper {
    pub fn from(bs: BytesMut, conn: Conn) -> Self {
        EarlyDataWrapper {
            ed: if bs.is_empty() { None } else { Some(bs) },
            base: Box::pin(conn),
        }
    }
}

impl Name for EarlyDataWrapper {
    fn name(&self) -> &'static str {
        "earlydata_wrapper_conn"
    }
}

impl AsyncRead for EarlyDataWrapper {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.ed.as_mut() {
            None => self.base.as_mut().poll_read(cx, buf),

            Some(ed) => {
                let el = ed.len();
                if el > 0 {
                    let m = min(el, buf.initialized().len());
                    //buf.set_filled(m);
                    buf.put(&ed[..m]);
                    ed.advance(m);
                    if ed.is_empty() {
                        self.ed = None;
                    }
                    Poll::Ready(Ok(()))
                } else {
                    self.ed = None;
                    self.base.as_mut().poll_read(cx, buf)
                }
            }
        }
    }
}

impl AsyncWrite for EarlyDataWrapper {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.base.as_mut().poll_write(cx, buf)
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

pub enum BytesDisplayMode {
    UTF8,
    Bytes,
}

/// It will print using debug! when reading or writing data.
pub struct PrintWrapper {
    base: Pin<Conn>,
    pub mode: BytesDisplayMode,
}

impl PrintWrapper {
    pub fn from(conn: Conn) -> Self {
        PrintWrapper {
            base: Box::pin(conn),
            mode: BytesDisplayMode::Bytes,
        }
    }
}

impl Name for PrintWrapper {
    fn name(&self) -> &'static str {
        "print_wrapper_conn"
    }
}

impl AsyncRead for PrintWrapper {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let r = self.base.as_mut().poll_read(cx, buf);

        match ready!(&r) {
            Ok(_) => {
                let slice = buf.filled();
                let sl = slice.len();
                debug!(
                    "read: {} {}",
                    sl,
                    String::from_utf8_lossy(&slice[..min(sl, 64)])
                )
            }
            Err(e) => {
                debug!("PrintWrapper read got e: {e}")
            }
        }

        r
    }
}

impl AsyncWrite for PrintWrapper {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let r = self.base.as_mut().poll_write(cx, buf);
        const MAX_DISPLAY_LEN: usize = 64;
        match ready!(&r) {
            Ok(n) => match self.mode {
                BytesDisplayMode::UTF8 => {
                    debug!(
                        "write: {}, {}",
                        *n,
                        String::from_utf8_lossy(&buf[..min(*n, MAX_DISPLAY_LEN)])
                    )
                }
                BytesDisplayMode::Bytes => {
                    let buf = crate::utils::HexSlice(&buf[..min(*n, MAX_DISPLAY_LEN)]);
                    let str = format!("{buf}");
                    debug!("write: {}, {str}", *n,)
                }
            },
            Err(e) => match self.mode {
                BytesDisplayMode::UTF8 => {
                    debug!(
                        "PrintWrapper write got e:{} {}, {e}",
                        buf.len(),
                        String::from_utf8_lossy(&buf[..min(buf.len(), MAX_DISPLAY_LEN)])
                    );
                }
                BytesDisplayMode::Bytes => {
                    let buf2 = crate::utils::HexSlice(buf);
                    let str = format!("{buf2}");
                    debug!("PrintWrapper write got e:{} {}, {e}", buf.len(), str,)
                }
            },
        };

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

#[derive(Default)]
pub enum BufferReadState {
    #[default]
    ReadyForNew,
    ContinueReadRemote {
        content_len: usize,
        body_start_index: usize,
        filled_data_len: usize,
    },
    ContinueReadLocalCache {
        from: usize,
        to: usize,
    },
    Closed,
}

/// Provide Buffered Reading for such protocols:
/// read the data head, and parse out a content_len,
/// the content_len may be later than the actually received data.
///
/// In that case, the read data need to be cached to get the whole data.
///
/// It needs a clousure "content_len_body_start_index_parse_fn" to be provided.
///
/// The real data must be right after the body_start_index returned by the clousure.
///
pub struct BufContentLenProtocolReader {
    read_cache: Option<BytesMut>,
    read_state: BufferReadState,
    read_cap: usize,
    reader: Pin<Box<dyn AsyncRead + Send + Sync>>,
    content_len_body_start_index_parse_fn:
        Box<dyn Fn(&[u8]) -> std::io::Result<(usize, usize)> + Send + Sync>,
}

/// returned by [`BufContentLenProtocolReader`]'s method `read`
///
/// It should not happen that `from >= to`. So `debug_assert!(from < to);` is recommended.

pub struct BufReadResult {
    pub buf: BytesMut,
    pub from: usize,
    pub to: usize,
}

impl BufContentLenProtocolReader {
    pub fn new(
        read_cap: usize,
        reader: Pin<Box<dyn AsyncRead + Send + Sync>>,
        content_len_body_start_index_parse_fn: Box<
            dyn Fn(&[u8]) -> std::io::Result<(usize, usize)> + Send + Sync,
        >,
    ) -> Self {
        BufContentLenProtocolReader {
            read_cache: None,
            read_state: Default::default(),
            read_cap,
            reader,
            content_len_body_start_index_parse_fn,
        }
    }

    pub fn is_closed(&self) -> bool {
        matches!(self.read_state, BufferReadState::Closed)
    }

    /// Call it after calling [`read`] and finished using the returned buffer.
    pub fn put_back(&mut self, rc: BytesMut) {
        let _ = self.read_cache.insert(rc);
    }

    /// if read succeed, it returns the buffer, the index where the data begins
    /// and the index where the data ends, wrapped in the struct [`BufReadResult`]
    ///
    /// After reading the buffer, the user must put it back using `self.put_back(buf)`.
    ///
    /// Also, it is required that the read method will not be called again before put back.
    ///
    /// This is implemented this way to avoid extra memory copy.
    ///
    /// It's not possible that `from >= to`.
    ///
    /// Ok(None) marks EOF.
    ///
    pub fn read(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<Option<BufReadResult>>> {
        let rc = self.read_cache.take();

        match self.read_state {
            BufferReadState::Closed => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "closed",
            ))),
            BufferReadState::ContinueReadLocalCache { from, to } => {
                debug_assert!(from < to);

                let rc = rc.unwrap();

                debug_assert_eq!(rc.len(), self.read_cap);

                self.read_header(cx, rc, from, to)
            }
            BufferReadState::ReadyForNew => {
                let mut rc = rc.unwrap_or(BytesMut::zeroed(self.read_cap));

                unsafe {
                    rc.set_len(self.read_cap);
                }

                let mut rb = ReadBuf::new(&mut rc);
                match self.reader.as_mut().poll_read(cx, &mut rb) {
                    Poll::Ready(r) => match r {
                        Ok(_) => {
                            let l = rb.filled().len();
                            if l == 0 {
                                self.read_state = BufferReadState::Closed;
                                tracing::trace!("bufprotocolreader read ok empty(EOF), will close");
                                return Poll::Ready(Ok(None));
                            }

                            self.read_header(cx, rc, 0, l)
                        }
                        Err(e) => {
                            let _ = self.read_cache.insert(rc);

                            Poll::Ready(Err(e))
                        }
                    },
                    Poll::Pending => {
                        let _ = self.read_cache.insert(rc);

                        Poll::Pending
                    }
                }
            }
            BufferReadState::ContinueReadRemote {
                content_len,
                body_start_index,
                filled_data_len,
            } => {
                let mut rc = rc.unwrap();

                debug_assert_eq!(self.read_cap, rc.len());

                let mut rb = ReadBuf::new(&mut rc);

                rb.set_filled(filled_data_len);

                match self.reader.as_mut().poll_read(cx, &mut rb) {
                    Poll::Pending => {
                        let _ = self.read_cache.insert(rc);

                        Poll::Pending
                    }
                    Poll::Ready(r) => match r {
                        Err(e) => {
                            let _ = self.read_cache.insert(rc);

                            Poll::Ready(Err(e))
                        }
                        Ok(_) => {
                            let data = rb.filled();
                            let dl = data.len();

                            debug_assert!(dl >= filled_data_len);

                            if dl == filled_data_len {
                                trace!("  ContinueReadRemote got empty(EOF), will close");
                                self.read_state = BufferReadState::Closed;
                                return Poll::Ready(Ok(None));
                            }

                            let real_len = data[body_start_index..].len();

                            if real_len < content_len {
                                tracing::trace!("partial read2: {real_len} {content_len}",);

                                self.read_state = BufferReadState::ContinueReadRemote {
                                    content_len,
                                    body_start_index,
                                    filled_data_len: dl,
                                };

                                let _ = self.read_cache.insert(rc);

                                self.read(cx)
                            } else {
                                if tracing::enabled!(tracing::Level::TRACE) {
                                    let real_data =
                                        &data[body_start_index..body_start_index + content_len];
                                    let real_string = String::from_utf8_lossy(real_data);

                                    tracing::trace!(
                                        "partial read2 finish, {}, {}, {}",
                                        real_len,
                                        content_len,
                                        real_string.len()
                                    );
                                }

                                if real_len > content_len {
                                    self.read_state = BufferReadState::ContinueReadLocalCache {
                                        from: body_start_index + content_len,
                                        to: dl,
                                    }
                                } else {
                                    self.read_state = BufferReadState::ReadyForNew;
                                }

                                Poll::Ready(Ok(Some(BufReadResult {
                                    buf: rc,
                                    from: body_start_index,
                                    to: body_start_index + content_len,
                                })))
                            }
                        }
                    },
                }
            }
        }
    }

    /// It extracts body len by using self.content_len_body_start_index_parse_fn, then
    /// if the cached `rc` is not less than the body len, it returns the result, or else
    /// it calls self.read to read until it got the full body.
    ///
    fn read_header(
        &mut self,
        cx: &mut Context<'_>,
        rc: BytesMut,
        from: usize,
        to: usize,
    ) -> Poll<std::io::Result<Option<BufReadResult>>> {
        debug_assert!(from < to);

        let data = &rc[from..to];
        let (content_len, body_start_index) = (self.content_len_body_start_index_parse_fn)(data)?;
        let real_len = data[body_start_index..].len();

        match content_len.cmp(&real_len) {
            std::cmp::Ordering::Less => {
                debug_assert!(from + body_start_index + content_len < to);

                self.read_state = BufferReadState::ContinueReadLocalCache {
                    from: from + body_start_index + content_len,
                    to,
                };

                Poll::Ready(Ok(Some(BufReadResult {
                    buf: rc,
                    from: from + body_start_index,
                    to: from + body_start_index + content_len,
                })))
            }
            std::cmp::Ordering::Equal => {
                self.read_state = BufferReadState::ReadyForNew;
                Poll::Ready(Ok(Some(BufReadResult {
                    buf: rc,
                    from: from + body_start_index,
                    to: from + body_start_index + content_len,
                })))
            }
            std::cmp::Ordering::Greater => {
                let mut new_rc = BytesMut::with_capacity(self.read_cap);

                new_rc.extend_from_slice(data);

                unsafe {
                    new_rc.set_len(self.read_cap);
                }

                self.read_state = BufferReadState::ContinueReadRemote {
                    content_len,
                    body_start_index,
                    filled_data_len: data.len(),
                };

                let _ = self.read_cache.insert(new_rc);

                self.read(cx)
            }
        }
    }
}

/// useful for testing
#[derive(Debug)]
pub struct MockTcpStream {
    pub read_data: Vec<u8>,
    pub write_data: Vec<u8>,
    pub write_target: Option<Arc<Mutex<Vec<u8>>>>,
}
impl crate::Name for MockTcpStream {
    fn name(&self) -> &str {
        "mock_tcp_stream"
    }
}

impl Unpin for MockTcpStream {}
impl AsyncRead for MockTcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        //debug!("MockTcp: read called");
        let size: usize = min(self.read_data.len(), buf.remaining());
        if size == 0 {
            return Poll::Ready(Ok(()));
        }
        buf.put(&self.read_data[..size]);

        let new_len = self.read_data.len() - size;

        self.read_data.copy_within(size.., 0);
        self.read_data.truncate(new_len);

        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for MockTcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        let mut x = Vec::from(buf);

        if let Some(swt) = &self.write_target {
            let mut v = swt.lock();
            v.append(&mut x);
        } else if self.write_data.is_empty() {
            self.write_data = x;
        } else {
            self.write_data.append(&mut x)
        }

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }
}

/// useful for testing
#[derive(Debug)]
pub struct MockTcpStream2<'a> {
    pub read_data: &'a mut Vec<u8>,
    pub write_data: &'a mut Vec<u8>,
    pub write_target: Option<Arc<Mutex<Vec<u8>>>>,
}
impl<'a> crate::Name for MockTcpStream2<'a> {
    fn name(&self) -> &str {
        "mock_tcp_stream2"
    }
}

impl<'a> Unpin for MockTcpStream2<'a> {}
impl<'a> AsyncRead for MockTcpStream2<'a> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        //debug!("MockTcp: read called");
        let size: usize = min(self.read_data.len(), buf.remaining());
        if size == 0 {
            return Poll::Ready(Ok(()));
        }

        buf.put(&self.read_data[..size]);

        let new_len = self.read_data.len() - size;

        self.read_data.copy_within(size.., 0);
        self.read_data.truncate(new_len);

        Poll::Ready(Ok(()))
    }
}

impl<'a> AsyncWrite for MockTcpStream2<'a> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        let mut x = Vec::from(buf);

        if let Some(swt) = &self.write_target {
            let mut v = swt.lock();
            v.append(&mut x);
        } else {
            self.write_data.append(&mut x)
        }

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use bytes::BufMut;
    use parking_lot::Mutex;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_ed_wrapper() -> std::io::Result<()> {
        let writev = Arc::new(Mutex::new(Vec::new()));

        let client_tcps = MockTcpStream {
            read_data: vec![111, 222, 123],
            write_data: Vec::new(),
            write_target: Some(writev),
        };

        let datalen = client_tcps.read_data.len();

        let mut buf = BytesMut::with_capacity(1024);
        let edslice = &[1, 2, 3, 4][..];
        buf.put_slice(edslice);
        assert_eq!(4, buf.len());

        let mut ed = helpers::EarlyDataWrapper::from(buf, Box::new(client_tcps));
        let mut nb = [0u8; 6];

        let r = ed.read(&mut nb).await;
        assert_eq!(r?, edslice.len());
        println!("{:?}", nb);

        let r = ed.read(&mut nb).await;
        assert_eq!(r?, datalen);
        println!("{:?}", nb);

        Ok(())
    }

    #[tokio::test]
    async fn test_ed_wrapper2() -> std::io::Result<()> {
        let writev = Arc::new(Mutex::new(Vec::new()));
        //let writevc = writev.clone();

        let client_tcps = MockTcpStream {
            read_data: vec![111, 222],
            write_data: Vec::new(),
            write_target: Some(writev),
        };

        let mut buf = BytesMut::with_capacity(1024);
        let edslice = &[1, 2, 3, 4][..];
        buf.put_slice(edslice);
        assert_eq!(4, buf.len());

        let mut ed = helpers::EarlyDataWrapper::from(buf, Box::new(client_tcps));

        let mut nb = [0u8; 3];

        let r = ed.read(&mut nb).await;
        assert_eq!(r?, 3);
        println!("{:?}", &nb[..3]);

        let r = ed.read(&mut nb).await;
        assert_eq!(r?, 1);
        println!("{:?}", &nb[..1]);

        let r = ed.read(&mut nb).await;
        assert_eq!(r?, 2);
        println!("{:?}", &nb[..2]);

        Ok(())
    }
}
