use core::time;
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use anyhow::bail;
use log::info;
use tokio::{io::ReadBuf, net::UdpSocket};

use self::map::{addr_conn::MAX_DATAGRAM_SIZE, helpers::MAX_LEN_SOCKS5_BYTES};

use super::*;
use crate::net::{
    self,
    addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr},
    *,
};

/// server side udp conn
#[derive(Clone)]
pub struct InboundConn {
    base: Arc<UdpSocket>,
}

impl InboundConn {
    pub fn new(u: UdpSocket) -> Self {
        InboundConn::newa(Arc::new(u))
    }

    pub fn newa(u: Arc<UdpSocket>) -> Self {
        InboundConn { base: u }
    }
}

pub fn new_addr_conn(u: UdpSocket) -> AddrConn {
    let a = Box::new(InboundConn::new(u));
    let b = a.clone();
    AddrConn::new(a, b)
}

impl AsyncWriteAddr for InboundConn {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let sor = addr.get_socket_addr_or_resolve();
        match sor {
            std::result::Result::Ok(so) => {
                let mut bf = BytesMut::with_capacity(buf.len() + MAX_LEN_SOCKS5_BYTES);

                encode_udp_diagram(
                    net::Addr {
                        addr: net::NetAddr::Socket(so),
                        network: net::Network::UDP,
                    },
                    &mut bf,
                );
                bf.extend_from_slice(buf);

                self.base.poll_send(cx, &mut bf)
            }
            Err(e) => Poll::Ready(Err(io::Error::other(e))),
        }
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(std::result::Result::Ok(()))
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(std::result::Result::Ok(()))
    }
}

fn decode_read(bs: &[u8]) -> anyhow::Result<(BytesMut, SocketAddr)> {
    let mut bf = BytesMut::from(bs);

    let a = decode_udp_diagram(&mut bf)?;

    let soa = a.get_socket_addr_or_resolve()?;

    Ok((bf, soa))
}

impl AsyncReadAddr for InboundConn {
    fn poll_read_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let mut new_buf = BytesMut::zeroed(MAX_DATAGRAM_SIZE);
        use std::result::Result::Ok;

        let mut rbuf = ReadBuf::new(&mut new_buf);
        let r = self.base.poll_recv_from(cx, &mut rbuf);
        match r {
            Poll::Pending => Poll::Pending,

            Poll::Ready(r) => match r {
                Err(e) => Poll::Ready(Err(e)),
                Ok(so) => {
                    let bs = rbuf.filled();

                    let r = decode_read(bs);

                    match r {
                        Ok((mut actual_buf, soa)) => {
                            actual_buf.copy_to_slice(buf);

                            Poll::Ready(Ok::<(usize, net::Addr), Error>((
                                actual_buf.len(),
                                crate::net::Addr {
                                    addr: NetAddr::Socket(soa),
                                    network: Network::UDP,
                                },
                            )))
                        }
                        Err(e) => Poll::Ready(Err(io::Error::other(e.to_string()))),
                    }
                }
            },
        }
    }
}

pub(super) async fn udp_associate(
    cid: CID,
    mut base: net::Conn,
    client_future_addr: net::Addr,
) -> anyhow::Result<MapResult> {
    let user_udp_socket = UdpSocket::bind("0.0.0.0:0").await?; //random port provided by OS.
    let udp_sock_addr = user_udp_socket.local_addr()?;
    let port = udp_sock_addr.port();

    //4个0为 BND.ADDR(4字节的ipv4) ,表示还是用原tcp的ip地址
    let reply = [
        VERSION5,
        SUCCESS,
        RSV,
        ATYP_IP4,
        0,
        0,
        0,
        0,
        (port >> 8) as u8, // BND.PORT(2字节)
        port as u8,
    ];
    base.write_all(&reply).await?;

    info!("socks5:{cid} listening a udp port for the user");

    let mut buf = BytesMut::zeroed(MAX_DATAGRAM_SIZE);

    let (_n, so) = tokio::time::timeout(
        time::Duration::from_secs(15),
        user_udp_socket.recv_from(&mut buf),
    )
    .await??;
    use std::result::Result::Ok;

    let cip = client_future_addr
        .get_ip()
        .expect("client_future_addr has ip");

    if !cip.is_unspecified() {
        if !so.ip().eq(&cip) {
            bail!("socks5 server udp listen for user first msg got msg other than user's ip addr, should from {}, but is from {}", so.ip(), cip)
        }
    }

    let ad = decode_udp_diagram(&mut buf)?;

    let ibc = new_addr_conn(user_udp_socket);
    let mr = MapResult::builder()
        .a(Some(ad))
        .b(Some(buf))
        .c(Stream::u(ibc));

    Ok(mr.build())
}
