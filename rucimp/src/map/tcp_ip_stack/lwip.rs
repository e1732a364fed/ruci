/*
! Implement tcp/ip stack by netstack_lwip;

<https://github.com/eycorsican/netstack-lwip/tree/master>

 */

use std::{fmt::Display, net::SocketAddr, pin::Pin};

use async_trait::async_trait;
use macro_map::{map_ext_fields, MapExt};

use ruci::map;
use ruci::{
    map::{Map, MapParams, MapResult, ProxyBehavior},
    net::CID,
};

use super::udp::{UdpRead, UdpWrite};
use super::Builder;

mod udp {

    use crate::map::tcp_ip_stack::udp::*;

    use super::*;

    #[async_trait::async_trait]
    impl UdpWrite for netstack_lwip::udp::SendHalf {
        async fn write(&mut self, data: DataDstSrc) -> std::io::Result<()> {
            self.send_to(&data.0, &data.1, &data.2)
        }
    }

    #[async_trait::async_trait]
    impl UdpRead for netstack_lwip::udp::RecvHalf {
        async fn read(&mut self) -> std::io::Result<(Vec<u8>, SocketAddr, SocketAddr)> {
            self.recv_from().await
        }
    }
}

#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct Stack {}

impl Display for Stack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lwip_stack")
    }
}

impl Builder for Stack {
    type AsyncConn = Pin<Box<netstack_lwip::TcpStream>>;
    type TcpConnStream = Pin<Box<netstack_lwip::TcpListener>>;
    type StackStream = Pin<Box<netstack_lwip::NetStack>>;

    fn build(
        &self,
    ) -> (
        Self::StackStream,
        Self::TcpConnStream,
        (Box<dyn UdpRead>, Box<dyn UdpWrite>),
    ) {
        let (stack, tcp_listener, udp_socket) = netstack_lwip::NetStack::new().unwrap();

        let (w, r) = udp_socket.split();
        (stack, tcp_listener, (Box::new(r), Box::new(w)))
    }
}

#[async_trait]
impl Map for Stack {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        crate::map::tcp_ip_stack::maps(cid, params, self).await
    }
}
