use std::{io, net::Ipv4Addr, pin::Pin, task::Poll};

use crate::Name;

use super::*;
use bytes::{Buf, BufMut, BytesMut};
use parking_lot::Mutex;
use tokio::io::ReadBuf;

use futures::task::Context;

use std::cmp::min;

///max len is 2 + 2 + 255 (domain)
pub const MAX_LEN_SOCKS5_BYTES: usize = 2 + 2 + 255;

//todo: add unit test
pub fn socks5_bytes_to_addr(buf: &mut BytesMut) -> anyhow::Result<Addr> {
    if buf.len() < 7 {
        return Err(anyhow!("socks5_bytes_to_addr length wrong1, {}", buf.len()));
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
                return Err(anyhow!("socks5_bytes_to_addr length wrong2, {}", buf.len()));
            }
            let num = buf.get_u32();
            ipn = IPName::IP(IpAddr::V4(Ipv4Addr::from(num)));
        }
        ATYP_IP6 => {
            if buf.len() < 18 {
                return Err(anyhow!("socks5_bytes_to_addr length wrong3, {}", buf.len()));
            }

            let num = buf.get_u128();
            ipn = IPName::IP(IpAddr::V6(Ipv6Addr::from(num)));
        }
        ATYP_DOMAIN => {
            if buf.len() < 4 {
                return Err(anyhow!("socks5_bytes_to_addr length wrong4, {}", buf.len()));
            }

            let dn = buf[0] as usize;
            buf.advance(1);

            if buf.len() < dn + 2 {
                return Err(anyhow!("socks5_bytes_to_addr length wrong5, {}", buf.len()));
            }
            ipn = IPName::Name(String::from_utf8_lossy(&buf[..dn]).to_string());
            buf.advance(dn);
        }
        _ => return Err(anyhow!("socks5_bytes_to_addr atyp wrong, {}", at)),
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

/// Wrap R: AsyncRead + Unpin,W: AsyncWrite + Unpin to an AsyncConn
pub struct RWWrapper<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> {
    pub r: R,
    pub w: W,
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

pub struct PrintWrapper {
    base: Pin<Conn>,
}

impl PrintWrapper {
    pub fn from(conn: Conn) -> Self {
        PrintWrapper {
            base: Box::pin(conn),
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

        match &r {
            Poll::Ready(r) => match r {
                Ok(_) => {
                    let slice = buf.filled();
                    let sl = slice.len();
                    debug!(
                        "read: {} {}",
                        sl,
                        String::from_utf8_lossy(&slice[..min(sl, 64)])
                    )
                }
                Err(_) => {}
            },
            Poll::Pending => {}
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

        match &r {
            Poll::Ready(r) => match r {
                Ok(u) => {
                    debug!(
                        "write: {} {}",
                        *u,
                        String::from_utf8_lossy(&buf[..min(*u, 64)])
                    )
                }
                Err(_) => {}
            },
            Poll::Pending => {}
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
        let size: usize = min(self.read_data.len(), buf.initialized().len());
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
        let size: usize = min(self.read_data.len(), buf.initialized().len());
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
