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

use super::tcp_ip_stack_common::Generator;

mod udp {
    use std::pin::Pin;

    use crate::map::tcp_ip_stack_common::udp::*;

    use super::*;

    pub(crate) struct U {
        pub udp: Option<Pin<Box<netstack_lwip::udp::UdpSocket>>>,
    }

    impl Splitter for U {
        fn split(&mut self) -> (Box<dyn Getter>, Box<dyn Putter>) {
            let (w, r) = self.udp.take().unwrap().split();
            (Box::new(r), Box::new(w))
        }
    }

    #[async_trait::async_trait]

    impl Putter for netstack_lwip::udp::SendHalf {
        async fn put(&mut self, data: DataDstSrc) -> std::io::Result<()> {
            self.send_to(&data.0, &data.1, &data.2)
        }
    }

    #[async_trait::async_trait]

    impl Getter for netstack_lwip::udp::RecvHalf {
        async fn get(&mut self) -> std::io::Result<(Vec<u8>, SocketAddr, SocketAddr)> {
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

impl Generator for Stack {
    type AsyncConn = Pin<Box<netstack_lwip::TcpStream>>;
    type TcpGetter = Pin<Box<netstack_lwip::TcpListener>>;

    type Stack = Pin<Box<netstack_lwip::NetStack>>;

    fn gen(
        &self,
    ) -> (
        Self::Stack,
        Self::TcpGetter,
        Box<dyn crate::map::tcp_ip_stack_common::udp::Splitter>,
    ) {
        let (stack, tcp_listener, udp_socket) = netstack_lwip::NetStack::new().unwrap();

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
