/*
! Implement tcp/ip stack by netstack_smoltcp;

<https://github.com/automesh-network/netstack-smoltcp>
 */

use std::fmt::Display;

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
    use std::net::SocketAddr;

    use crate::map::tcp_ip_stack::udp::{DataDstSrc, UdpRead, UdpWrite};

    #[async_trait::async_trait]
    impl UdpWrite for netstack_smoltcp::udp::WriteHalf {
        async fn write(&mut self, data: DataDstSrc) -> std::io::Result<()> {
            use futures::SinkExt;
            self.send(data).await
        }
    }

    #[async_trait::async_trait]
    impl UdpRead for netstack_smoltcp::udp::ReadHalf {
        async fn read(&mut self) -> std::io::Result<(Vec<u8>, SocketAddr, SocketAddr)> {
            use futures::StreamExt;
            let r = self.next().await;
            r.ok_or(std::io::Error::other("smoltcp udp ReadHalf got None"))
        }
    }
}

#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct Stack {}

impl Display for Stack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "smoltcp_stack")
    }
}

impl Builder for Stack {
    type AsyncConn = netstack_smoltcp::TcpStream;
    type TcpConnStream = netstack_smoltcp::TcpListener;
    type StackStream = netstack_smoltcp::Stack;

    fn build(
        &self,
    ) -> (
        Self::StackStream,
        Self::TcpConnStream,
        (Box<dyn UdpRead>, Box<dyn UdpWrite>),
    ) {
        let (stack, runner, udp_socket, tcp_listener) = netstack_smoltcp::StackBuilder::default()
            .stack_buffer_size(512)
            .tcp_buffer_size(4096)
            .enable_udp(true)
            .enable_tcp(true)
            .enable_icmp(true)
            .build()
            .unwrap();
        let tcp_listener = tcp_listener.unwrap();
        let udp_socket = udp_socket.unwrap();
        if let Some(runner) = runner {
            tokio::spawn(runner);
        }

        let (r, w) = udp_socket.split();

        (stack, tcp_listener, (Box::new(r), Box::new(w)))
    }
}

#[async_trait]
impl Map for Stack {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        crate::map::tcp_ip_stack::maps(cid, params, self).await
    }
}
