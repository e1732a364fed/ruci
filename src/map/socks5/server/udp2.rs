use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use tokio::{io::ReadBuf, net::UdpSocket};

use super::*;
use crate::net::{
    self,
    addr_conn::{AddrConn, AsyncReadAddr, AsyncWriteAddr},
    *,
};

#[derive(Clone)]
pub struct Conn {
    base: Arc<UdpSocket>,
}

impl Conn {
    pub fn new(u: UdpSocket) -> Self {
        Conn { base: Arc::new(u) }
    }

    pub fn newa(u: Arc<UdpSocket>) -> Self {
        Conn { base: u }
    }

    pub fn duplicate(&self) -> Conn {
        self.clone()
    }
}

pub fn new(u: UdpSocket) -> AddrConn {
    let a = Arc::new(u);
    let b = a.clone();
    AddrConn::new(Box::new(Conn::newa(a)), Box::new(Conn::newa(b)))
}

/// wrap u with Arc, then return 2 copys.
pub fn duplicate(u: UdpSocket) -> (Conn, Conn) {
    let a = Arc::new(u);
    let b = a.clone();
    (Conn::newa(a), Conn::newa(b))
}

impl AsyncWriteAddr for Conn {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let sor = addr.get_socket_addr_or_resolve();
        match sor {
            std::result::Result::Ok(so) => self.base.poll_send_to(cx, buf, so),
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

impl AsyncReadAddr for Conn {
    fn poll_read_addr(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let mut rbuf = ReadBuf::new(buf);
        let r = self.base.poll_recv_from(cx, &mut rbuf);
        match r {
            Poll::Ready(r) => match r {
                std::result::Result::Ok(so) => Poll::Ready(std::result::Result::Ok((
                    rbuf.filled().len(),
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

pub(super) async fn udp_associate(
    cid: CID,
    mut base: net::Conn,
    client_future_addr: net::Addr,
) -> anyhow::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?; //random port provided by OS.
    let udp_sock_addr = socket.local_addr()?;
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

    task::spawn(loop_listen_udp_for_certain_client(
        cid,
        base,
        client_future_addr,
        socket,
    ));

    Ok(())
}

pub async fn loop_listen_udp_for_certain_client(
    cid: CID,
    mut base: net::Conn,
    client_future_addr: net::Addr,
    udpso_created_to_listen_for_thisuser: UdpSocket,
) -> anyhow::Result<()> {
    const CAP: usize = 1500; //todo: change this

    let mut buf2 = BytesMut::with_capacity(CAP);

    let mut user_raddr: IpAddr = client_future_addr
        .get_ip()
        .expect("client_future_addr has ip");
    let mut user_port: u16 = client_future_addr.get_port();

    use futures::FutureExt;
    use tokio::sync::mpsc::channel;
    let (tx, mut rx) = channel::<(Option<SocketAddr>, BytesMut, SocketAddr)>(20);

    let udpso_created_to_listen_for_thisuser = Arc::new(udpso_created_to_listen_for_thisuser);

    let udp = udpso_created_to_listen_for_thisuser.clone();

    //loop write to user or remote
    task::spawn(async move {
        let mut buf_w = BytesMut::with_capacity(CAP);

        loop {
            let x = rx.recv().await;
            match x {
                None => break,

                Some((msg_was_from, buf, send_to)) => {
                    match msg_was_from {
                        None => {
                            // from the user, to remote. the buf is already decoded
                            if udp.send_to(&buf, send_to).await.is_err() {
                                break;
                            }
                        }
                        Some(from) => {
                            // from a remote, to the user. the send_to is the user's address

                            buf_w.clear();
                            encode_udp_diagram(
                                net::Addr {
                                    addr: net::NetAddr::Socket(from),
                                    network: net::Network::UDP,
                                },
                                &mut buf_w,
                            );

                            buf_w.extend_from_slice(&buf);

                            let r = udp.send_to(&buf_w, send_to).await;

                            if r.is_err() {
                                break;
                            }
                        }
                    } //match
                } //Some
            } //match
        } // loop
    }); //task

    //loop read from user or remote
    loop {
        select! {

            /*
            A UDP association terminates when the TCP connection that the UDP
            ASSOCIATE request arrived on terminates.
            */

            result = base.read(&mut buf2).fuse()  =>{
                if let Err(e ) = result{
                    debug!("{}, socks5 server, will end loop listen udp because of the read err of the tcp conn, {}", cid, e);

                    drop(tx);

                    break;
                }

                warn!("{cid}, socks5 server, tcp conn got read data, but we don't know what to do with it", );

            },
            default =>{
                let mut buf = BytesMut::zeroed(CAP);

                let (n, raddr) = udpso_created_to_listen_for_thisuser.recv_from(&mut buf).await?;
                buf.truncate(n);


                if user_raddr.is_unspecified() || raddr.ip().eq(&user_raddr) {

                    //user write to remote

                    if user_raddr.is_unspecified() {
                        user_raddr = raddr.ip();
                        user_port = raddr.port();
                    }

                    let a = decode_udp_diagram(&mut buf)?;

                    let so = match a.get_socket_addr_or_resolve(){
                        std::result::Result::Ok(s) => s,
                        Err(e) => {
                            warn!("can't convert to socketaddr, {}",e);
                            continue;
                        },

                    };

                    let x= tx.send(( None,buf, so)).await;
                    if let Err(e) = x{
                        return Err(anyhow!("{}",e));
                    }


                }else{

                    // new data from some remote

                    let r= tx.send(( Some(raddr),buf,SocketAddr::new(user_raddr,user_port))).await;
                    if let Err(e) = r{
                        return Err(anyhow!("{}",e));
                    }
                }
            }
        }
    }

    Ok(())
}
