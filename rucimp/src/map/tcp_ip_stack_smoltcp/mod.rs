/*
! Implement tcp/ip stack by netstack_smoltcp;

<https://github.com/automesh-network/netstack-smoltcp>

The mod is a mirror of mod tcp_ip_stack_lwip.

 */

use std::fmt::Display;

use async_trait::async_trait;
use macro_map::{map_ext_fields, MapExt};
use ruci::map;
use ruci::{
    map::{Map, MapParams, MapResult, ProxyBehavior},
    net::CID,
};

use super::tcp_ip_stack_common::Generator;

mod udp {
    use std::net::SocketAddr;

    use crate::map::tcp_ip_stack_common::udp::{DataDstSrc, Getter, Putter, Splitter};

    #[async_trait::async_trait]

    impl Putter for netstack_smoltcp::udp::WriteHalf {
        async fn put(&mut self, data: DataDstSrc) -> std::io::Result<()> {
            use futures::SinkExt;
            self.send(data).await
        }
    }

    #[async_trait::async_trait]

    impl Getter for netstack_smoltcp::udp::ReadHalf {
        async fn get(&mut self) -> std::io::Result<(Vec<u8>, SocketAddr, SocketAddr)> {
            use futures::StreamExt;
            let r = self.next().await;
            r.ok_or(std::io::Error::other("smoltcp udp ReadHalf got None"))
        }
    }

    pub(crate) struct U {
        pub udp: Option<netstack_smoltcp::udp::UdpSocket>,
    }

    impl Splitter for U {
        fn split(&mut self) -> (Box<dyn Getter>, Box<dyn Putter>) {
            let (r, w) = self.udp.take().unwrap().split();
            (Box::new(r), Box::new(w))
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

impl Generator for Stack {
    type AsyncConn = netstack_smoltcp::TcpStream;
    type TcpGetter = netstack_smoltcp::TcpListener;

    type Stack = netstack_smoltcp::Stack;

    fn gen(
        &self,
    ) -> (
        Self::Stack,
        Self::TcpGetter,
        Box<dyn crate::map::tcp_ip_stack_common::udp::Splitter>,
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

        (
            stack,
            tcp_listener,
            Box::new(udp::U {
                udp: Some(udp_socket),
            }),
        )
    }
}

#[async_trait]
impl Map for Stack {
    async fn maps(&self, cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        crate::map::tcp_ip_stack_common::maps(cid, behavior, params, self).await
    }
}
