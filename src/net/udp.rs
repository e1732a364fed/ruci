/*!
Implements traits in mod [`crate::net::addr_conn`] for [`tokio::net::UdpSocket`] , and the resulting structure is [`Conn`].

*/
use crate::utils::io_error;

use super::addr_conn::{AsyncReadAddr, AsyncWriteAddr};
use super::*;
use std::io;
use std::task::ready;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{io::ReadBuf, net::UdpSocket};

#[derive(Default, Clone, Copy, Debug)]
pub enum Mode {
    #[default]
    Dial,

    FixTargetListen,
}

/// Implements AddrConn trait
///
/// 固定用同一个 udp socket 发送, 到不同的远程地址也是如此
#[derive(Clone)]
pub struct Conn {
    pub last_laddr: Option<Arc<parking_lot::RwLock<Addr>>>,

    pub mode: Mode,

    pub opt_dns_client: Option<Arc<dns::AsyncClient>>,

    u: Arc<UdpSocket>,
    peer_addr: Option<Addr>,
}
impl Display for Conn {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "udp")
    }
}

impl Conn {
    /// init a Conn from a UdpSocket
    ///
    /// 如果 peer_addr 给出, 说明 u 是 connected, 将用 recv 而不是 recv_from,
    /// 以及用 send 而不是 send_to
    ///
    pub fn new(u: UdpSocket, peer_addr: Option<Addr>, fix_target_listen: bool) -> Self {
        let mode = if fix_target_listen {
            Mode::FixTargetListen
        } else {
            Mode::Dial
        };

        Conn {
            u: Arc::new(u),
            peer_addr,
            last_laddr: None,
            mode,
            opt_dns_client: None,
        }
    }
}

/// init a AddrConn from a UdpSocket
///
/// 如果 peer_addr 给出, 说明 u 是 connected, 将用 recv 而不是 recv_from,
/// 以及用 send 而不是 send_to
///
pub fn new(
    u: UdpSocket,
    peer_addr: Option<Addr>,
    fix_target_listen: bool,
    oc: Option<Arc<dns::AsyncClient>>,
) -> AddrConn {
    let a = Arc::new(u);
    let b = a.clone();
    let mode = if fix_target_listen {
        Mode::FixTargetListen
    } else {
        Mode::Dial
    };

    let last_laddr: Option<Arc<parking_lot::RwLock<Addr>>> = match mode {
        Mode::Dial => None,
        Mode::FixTargetListen => Some(Arc::new(parking_lot::RwLock::new(Addr::default()))),
    };

    let c1 = Conn {
        last_laddr: last_laddr.clone(),
        mode,
        u: a,
        peer_addr: peer_addr.clone(),
        opt_dns_client: oc.clone(),
    };
    let c2 = Conn {
        u: b,
        peer_addr,
        last_laddr,
        mode,
        opt_dns_client: oc.clone(),
    };
    AddrConn::new(Box::new(c1), Box::new(c2))
}

/// wrap u with Arc, then return 2 copies.
pub fn duplicate(u: UdpSocket) -> (Conn, Conn) {
    let a = Arc::new(u);
    let b = a.clone();
    (
        Conn {
            u: a,
            peer_addr: None,
            last_laddr: None,
            mode: Mode::Dial,
            opt_dns_client: None,
        },
        Conn {
            u: b,
            peer_addr: None,
            last_laddr: None,
            mode: Mode::Dial,
            opt_dns_client: None,
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
        //trace!("udp write called {} {addr} {:?}", buf.len(), self.peer_addr);
        if self.peer_addr.is_some() || addr.eq(&Addr::default()) {
            self.u.poll_send(cx, buf)
        } else {
            let sor_f = addr.get_socket_addr_or_resolve(self.opt_dns_client.as_deref());
            let pr = std::future::Future::poll(std::pin::pin!(sor_f), cx);
            match ready!(pr) {
                Ok(so) => {
                    if let Mode::FixTargetListen = self.mode {
                        let laddr = match &self.last_laddr {
                            Some(a) => a.read().get_socket_addr().unwrap(),
                            None => {
                                return Poll::Ready(Err(io_error(
                                    "udp write mode FixTargetListen, no last_laddr",
                                )))
                            }
                        };
                        self.u.poll_send_to(cx, buf, laddr)
                    } else {
                        self.u.poll_send_to(cx, buf, so)
                    }
                }
                Err(e) => Poll::Ready(Err(io::Error::other(e))),
            }
        }
    }
}

impl AsyncReadAddr for Conn {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let mut r_buf = ReadBuf::new(buf);
        if let Some(pa) = self.peer_addr.as_ref() {
            let r = self.u.poll_recv(cx, &mut r_buf);
            match ready!(r) {
                Ok(_) => {
                    let r_len = r_buf.filled().len();
                    //trace!("udp with peer_addr read got {}", r_len);

                    Poll::Ready(Ok((r_len, pa.clone())))
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        } else {
            let r = self.u.poll_recv_from(cx, &mut r_buf);
            match ready!(r) {
                Ok(so) => {
                    let r_len = r_buf.filled().len();
                    //trace!("udp read got {} {so}", r_len);

                    let addr = crate::net::Addr {
                        addr: NetAddr::Socket(so),
                        network: Network::UDP,
                    };
                    if let Mode::FixTargetListen = self.mode {
                        let mut mg = self.last_laddr.as_mut().unwrap().write();
                        *mg = addr.clone()
                    }

                    Poll::Ready(Ok((r_len, addr)))
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        }
    }
}

#[cfg(test)]
#[allow(unused)]
mod test {
    use futures::pin_mut;
    use futures::select;
    use futures::FutureExt;
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

    impl Display for MockStream {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "mock_stream")
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

            let mut bv = Vec::from(buf);

            if let Some(swt) = &self.write_target {
                let mut v = swt.lock();
                v.append(&mut bv);
            } else if self.write_data.is_empty() {
                self.write_data = bv;
            } else {
                self.write_data.append(&mut bv)
            }

            Poll::Ready(Ok(buf.len()))
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

    /// 循环读5遍后退出
    async fn read_timeout(name: String, mut c: super::Conn) -> io::Result<()> {
        let mut buf = [0u8; CAP];
        let mut wbuf = [0u8, 2, 2, 3, 4];

        let nc = name.clone();
        let f1 = async move {
            let mut count = 1;
            loop {
                if count > 5 {
                    return Ok::<(), io::Error>(());
                }
                let (n, ad) = c.read(&mut buf).await?;
                println!("{} read from,{} {:?}", nc.as_str(), ad, &buf[..n]);

                // c.write(src)
                c.write(&wbuf, &ad).await;
                count += 1;
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
        let u1 = UdpSocket::bind("127.0.0.1:23456").await?;

        let u2_addr_str = "127.0.0.1:34567";
        let u2 = UdpSocket::bind(u2_addr_str).await?;
        let (mut r1, mut w1) = duplicate(u1);
        let (mut r2, mut w2) = duplicate(u2);

        let r1 = tokio::task::spawn(read_timeout("1".to_string(), r1));

        let r2 = tokio::task::spawn(read_timeout("2".to_string(), r2));

        let w1 = tokio::task::spawn(async move {
            let mut buf = [0u8, 1, 2, 3, 4];
            let ta_u2 = crate::net::Addr {
                addr: NetAddr::Socket(SocketAddr::from_str(u2_addr_str).unwrap()),
                network: Network::TCP,
            };
            let mut i = 0;
            while i != 5 {
                let n = w1.write(&buf, &ta_u2).await?;
                println!("w write to,{} {:?}", &ta_u2, &buf[..n]);

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
    async fn test_addrconn_cp1() -> io::Result<()> {
        // 从 u1的w 写入 u2的 r，之后用 addr_conn::cp_addr 从 u2的 r 拷贝到 mock_stream1的 write_target

        let u1 = UdpSocket::bind("127.0.0.1:12346").await?;

        let ad2_str = "127.0.0.1:23457";
        let u2 = UdpSocket::bind(ad2_str).await?;

        let (_r, mut w) = duplicate(u1);
        let (r2, _w2) = duplicate(u2);

        let writev = Arc::new(Mutex::new(Vec::new()));
        let writevc = writev.clone();

        let mock_stream1 = MockStream {
            read_data: Vec::new(),
            write_data: Vec::new(),
            write_target: Some(writev),
        };
        let mut buf_to_write = [0u8, 1, 2, 3, 4];

        let _w1 = tokio::task::spawn(async move {
            let ta = crate::net::Addr {
                addr: NetAddr::Socket(
                    SocketAddr::from_str(ad2_str)
                        .map_err(|e| io::Error::other(format!("{}", e)))?,
                ),
                network: Network::TCP,
            };
            let mut i = 0;
            while i != 5 {
                i += 1;

                let n = w.write(&buf_to_write, &ta).await?;
                println!("w write to,{} {:?}", &ta, &buf_to_write[..n]);

                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            println!("w2 end");

            Ok::<(), io::Error>(())
        });
        let (_tx, rx) = oneshot::channel();

        let _ = crate::net::addr_conn::cp_addr(
            CID::default(),
            r2,
            mock_stream1,
            // "".to_string(),
            false,
            rx,
            false,
            None,
        )
        .await;

        let data = buf_to_write.repeat(5);

        print!("test: cp addr end");

        assert_eq!(&data, writevc.lock().deref());
        Ok(())
    }
}
