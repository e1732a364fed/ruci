use core::time;

use anyhow::bail;
use log::info;
use tokio::net::UdpSocket;

use self::map::addr_conn::MAX_DATAGRAM_SIZE;

use super::*;
use crate::{
    map::socks5::udp::new_addr_conn,
    net::{self, *},
};

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

    let inbound_c = new_addr_conn(
        user_udp_socket,
        client_future_addr
            .get_socket_addr()
            .expect("should have correct socketAddr"),
    );
    let mr = MapResult::builder()
        .a(Some(ad))
        .b(Some(buf))
        .c(Stream::u(inbound_c));

    Ok(mr.build())
}
