/*!
 * 为 UdpSocket 实现 net::addr_conn 中的trait

*/
use super::addr_conn::{AsyncReadAddr, AsyncWriteAddr};
use super::*;
use std::io;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{io::ReadBuf, net::UdpSocket};

/// Implements AddrConn trait
///
/// 固定用同一个 udp socket 发送, 到不同的远程地址也是如此
#[derive(Clone)]
pub struct Conn {
    u: Arc<UdpSocket>,
    peer_addr: Option<Addr>,
}
impl crate::Name for Conn {
    fn name(&self) -> &str {
        "udp"
    }
}

impl Conn {
    /// init a Conn from a UdpSocket
    ///
    /// 如果 peer_addr 给出, 说明 u 是 connected, 将用 recv 而不是 recv_from,
    /// 以及用 send 而不是 send_to
    ///
    pub fn new(u: UdpSocket, peer_addr: Option<Addr>) -> Self {
        Conn {
            u: Arc::new(u),
            peer_addr,
        }
    }
}

/// init a AddrConn from a UdpSocket
///
/// 如果 peer_addr 给出, 说明 u 是 connected, 将用 recv 而不是 recv_from,
/// 以及用 send 而不是 send_to
///
pub fn new(u: UdpSocket, peer_addr: Option<Addr>) -> AddrConn {
    let a = Arc::new(u);
    let b = a.clone();
    AddrConn::new(
        Box::new(Conn {
            u: a,
            peer_addr: peer_addr.clone(),
        }),
        Box::new(Conn { u: b, peer_addr }),
    )
}

/// wrap u with Arc, then return 2 copies.
pub fn duplicate(u: UdpSocket) -> (Conn, Conn) {
    let a = Arc::new(u);
    let b = a.clone();
    (
        Conn {
            u: a,
            peer_addr: None,
        },
        Conn {
            u: b,
            peer_addr: None,
        },
    )
}

impl AsyncWriteAddr for Conn {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        if self.peer_addr.is_some() || addr.eq(&Addr::default()) {
            self.u.poll_send(cx, buf)
        } else {
            let sor = addr.get_socket_addr_or_resolve();
            match sor {
                Ok(so) => self.u.poll_send_to(cx, buf, so),
                Err(e) => Poll::Ready(Err(io::Error::other(e))),
            }
        }
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncReadAddr for Conn {
    fn poll_read_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let mut r_buf = ReadBuf::new(buf);
        if let Some(pa) = self.peer_addr.as_ref() {
            let r = self.u.poll_recv(cx, &mut r_buf);
            match r {
                Poll::Ready(r) => match r {
                    Ok(_) => Poll::Ready(Ok((r_buf.filled().len(), pa.clone()))),
                    Err(e) => Poll::Ready(Err(e)),
                },
                Poll::Pending => Poll::Pending,
            }
        } else {
            let r = self.u.poll_recv_from(cx, &mut r_buf);
            match r {
                Poll::Ready(r) => match r {
                    Ok(so) => Poll::Ready(Ok((
                        r_buf.filled().len(),
                        crate::net::Addr {
                            addr: NetAddr::Socket(so),
                            network: Network::UDP,
                        },
                    ))),
                    Err(e) => Poll::Ready(Err(e)),
                },
                Poll::Pending => Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
#[allow(unused)]
mod test {
    use futures::select;
    use futures_util::join;
    use parking_lot::Mutex;
    use tokio::sync::oneshot;

    use super::*;
    use crate::net::addr_conn::{AsyncReadAddrExt, AsyncWriteAddrExt};
    use std::{cmp::min, io, ops::Deref, str::FromStr, time::Duration};

    const CAP: usize = 1500;

    #[derive(Debug)]
    pub struct MockStream {
        pub read_data: Vec<u8>,
        pub write_data: Vec<u8>,
        pub write_target: Option<Arc<Mutex<Vec<u8>>>>,
    }
    impl crate::Name for MockStream {
        fn name(&self) -> &str {
            "mock_stream"
        }
    }

    impl AsyncWriteAddr for MockStream {
        fn poll_write_addr(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
            _addr: &Addr,
        ) -> Poll<io::Result<usize>> {
            //debug!("MockUdp: write called");

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

        fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncReadAddr for MockStream {
        fn poll_read_addr(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<io::Result<(usize, Addr)>> {
            //debug!("MockUdp: read called");

            let cp_size: usize = min(self.read_data.len(), buf.len());
            buf.copy_from_slice(&self.read_data[..cp_size]);

            let new_len = self.read_data.len() - cp_size;

            self.read_data.copy_within(cp_size.., 0);
            self.read_data.truncate(new_len);

            Poll::Ready(Ok((cp_size, crate::net::Addr::default())))
        }
    }

    async fn read_timeout(name: String, mut r: Conn) -> io::Result<()> {
        let mut buf = [0u8; CAP];

        let nc = name.clone();
        let f1 = async move {
            loop {
                let (n, ad) = r.read(&mut buf).await?;
                println!("{} read from,{} {:?}", nc.as_str(), ad, &buf[..n]);
            }
            Ok::<(), io::Error>(())
        }
        .fuse();

        // read udp must combined with select, or it will never ends

        let sleep_f = tokio::time::sleep(Duration::from_secs(10)).fuse();
        pin_mut!(f1, sleep_f);

        select! {
            x1 = f1 =>{
                println!("{} read end in select,", &name);
            }
            x2 = sleep_f =>{
                println!("{} read timeout in select",&name);

            }

        }

        println!("{} end", name.as_str(),);

        Ok::<(), io::Error>(())
    }

    #[tokio::test]
    async fn test_udp_rw() -> io::Result<()> {
        let u = UdpSocket::bind("127.0.0.1:23456").await?;
        let u2 = UdpSocket::bind("127.0.0.1:34567").await?;
        let (mut r, mut w) = duplicate(u);
        let (mut r2, mut w2) = duplicate(u2);

        let r1 = tokio::task::spawn(read_timeout("1".to_string(), r));

        let r2 = tokio::task::spawn(read_timeout("2".to_string(), r2));

        let w1 = tokio::task::spawn(async move {
            let mut buf = [0u8, 1, 2, 3, 4];
            let ta = crate::net::Addr {
                addr: NetAddr::Socket(
                    SocketAddr::from_str("127.0.0.1:34567")
                        .map_err(|x| io::Error::other(format!("{}", x)))?,
                ),
                network: Network::TCP,
            };
            let mut i = 0;
            while i != 5 {
                let n = w.write(&mut buf, &ta).await?;
                println!("w write to,{} {:?}", &ta, &buf[..n]);

                tokio::time::sleep(Duration::from_secs(1)).await;

                i += 1;
            }
            println!("w2 end");

            Ok::<(), io::Error>(())
        });

        join!(w1, r1, r2);
        println!("join end");

        Ok(())
    }

    /// test the auto timeout feature in addrconn
    /// it will write a data once per second for 5 times,
    ///
    /// then it should hung for CP_UDP_TIMEOUT of time, then returns.
    ///
    #[tokio::test]
    async fn test_addrconn_cp() -> io::Result<()> {
        let u = UdpSocket::bind("127.0.0.1:12346").await?;

        let ad2_str = "127.0.0.1:23457";
        let u2 = UdpSocket::bind(ad2_str).await?;

        let (_r, mut w) = duplicate(u);
        let (r2, _w2) = duplicate(u2);

        let writev = Arc::new(Mutex::new(Vec::new()));
        let writevc = writev.clone();

        let ms = MockStream {
            read_data: Vec::new(),
            write_data: Vec::new(),
            write_target: Some(writev),
        };
        let mut buf_to_write = [0u8, 1, 2, 3, 4];

        let _w1 = tokio::task::spawn(async move {
            let ta = crate::net::Addr {
                addr: NetAddr::Socket(
                    SocketAddr::from_str(ad2_str)
                        .map_err(|x| io::Error::other(format!("{}", x)))?,
                ),
                network: Network::TCP,
            };
            let mut i = 0;
            while i != 5 {
                i += 1;

                let n = w.write(&mut buf_to_write, &ta).await?;
                println!("w write to,{} {:?}", &ta, &buf_to_write[..n]);

                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            println!("w2 end");

            Ok::<(), io::Error>(())
        });
        // let (_, rx) = oneshot::channel();
        let (tx1, rx1) = tokio::sync::broadcast::channel(10);

        let _ = crate::net::addr_conn::cp_addr(r2, ms, "".to_string(), false, tx1).await;

        let nv = buf_to_write.repeat(5);

        print!("test: cp addr end");

        assert_eq!(&nv, writevc.lock().deref());
        Ok(())
    }
}
