/*
! Implement tcp/ip stack by netstack_lwip;

<https://github.com/eycorsican/netstack-lwip/tree/master>

 */

use std::{
    future::Future,
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    pin::Pin,
    sync::{atomic::AtomicU32, Arc},
    task::{Context, Poll},
};

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use macro_map::{map_ext_fields, MapExt};
use netstack_lwip::{
    udp::{RecvHalf, SendHalf},
    NetStack,
};
use ruci::{
    map,
    net::{addr_conn::AddrConn, tun, Network},
};
use ruci::{
    map::{Map, MapParams, MapResult, ProxyBehavior},
    net::{
        addr_conn::{AsyncReadAddr, AsyncWriteAddr},
        Addr, CID,
    },
    Name,
};
use tokio::sync::mpsc;
use tracing::warn;
use tracing::{debug, info};

#[cfg(feature = "tun")]
#[derive(Clone, Debug, Default)]
pub enum AutoRouteState {
    #[default]
    None,
    InUp(Option<Vec<String>>), //old_dns_list
    Down,
}

#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct Stack {
    pub addr: ruci::net::Addr,

    #[cfg(feature = "tun")]
    pub in_auto_route: Option<tun::route::InAutoRouteParams>,

    #[cfg(feature = "tun")]
    pub auto_route_state: Arc<parking_lot::Mutex<AutoRouteState>>,
}

impl Name for Stack {
    fn name(&self) -> &'static str {
        "smoltcp_stack"
    }
}

#[cfg(feature = "tun")]
impl Drop for Stack {
    fn drop(&mut self) {
        self.down_route();
    }
}

impl Stack {
    #[cfg(feature = "tun")]
    pub fn down_route(&mut self) {
        use ruci::net::tun;

        let mut mg = self.auto_route_state.lock();
        match &*mg {
            AutoRouteState::InUp(opt_dns_list) => {
                debug!("Stack down in auto route");

                let mut params = self.in_auto_route.clone().unwrap();
                params.dns_list = opt_dns_list.to_owned();
                let r = tun::route::in_down_route(&params);
                debug!("Stack down in auto route {r:?}");
                if r.is_ok() {
                    *mg = AutoRouteState::Down;
                }
            }

            _ => {}
        }
    }
}

#[async_trait]
impl Map for Stack {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let conn = params.c;
        if let ruci::net::Stream::None = conn {
            if let Some(c) = &self.in_auto_route {
                let mut mg = self.auto_route_state.lock();
                match &*mg {
                    AutoRouteState::InUp(_) => {
                        info!("Stack called after AutoRouteState::InUp")
                    }
                    _ => {
                        let r = tun::route::in_auto_route(c);
                        match r {
                            Ok(opt_dns_list) => {
                                *mg = AutoRouteState::InUp(opt_dns_list);
                            }
                            Err(e) => {
                                return MapResult::from_e(e.context("Stack in auto_route failed"))
                            }
                        }
                    }
                }
            }

            let addr = &self.addr;

            let (tun_name, dial_addr, netmask) = addr.to_name_ip_netmask().unwrap();

            let (stack, mut tcp_listener, udp_socket) = NetStack::new().unwrap();
            let (mut stack_sink, mut stack_stream) = stack.split();

            debug!("try init with {:?},{},{:?}", tun_name, dial_addr, netmask);
            let r = ruci::net::tun::create_bind_sink_stream(tun_name, dial_addr, netmask).await;

            if r.is_err() {
                if let Err(e) = r {
                    return MapResult::from_e(e);
                }
            }

            let (mut tun_sink, mut tun_stream) = r.unwrap();

            // Reads packet from TUN and sends to stack.
            tokio::spawn(async move {
                while let Some(pkt) = tun_stream.next().await {
                    debug!("tun got pkt {:?}", pkt);
                    if let Ok(pkt) = pkt {
                        stack_sink.send(pkt).await.unwrap();
                    }
                }
                debug!("end2");
            });

            // Reads packet from stack and sends to TUN.
            tokio::spawn(async move {
                while let Some(pkt) = stack_stream.next().await {
                    debug!("stack got pkt {:?}", pkt);

                    if let Ok(pkt) = pkt {
                        tun_sink.send(pkt).await.unwrap();
                    }
                }
                debug!("end1");
            });

            let (tx, rx) = mpsc::channel(100); //todo adjust this

            let cc = cid.clone();

            tokio::spawn(async move {
                let s_count: AtomicU32 = AtomicU32::new(1);

                // let (w, r) = udp_socket.split();
                // let ar = AddrConnR { base: r };
                // let aw = AddrConnW {
                //     base: w,
                //     src_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
                // };
                // let ac: AddrConn = AddrConn {
                //     r: Box::new(ar),
                //     w: Box::new(aw),
                //     default_write_to: None,
                //     cached_name: String::from(""),
                // };
                // let mut a = Addr::default();
                // a.network = Network::UDP;
                // let m = MapResult::new_u(ac).a(Some(a)).build();
                // let r = tx.send(m).await;
                // if let Err(e) = r {
                //     warn!(cid = %cc, "stack send tx got error: {}", e);
                // }

                while let Some((stream, local_addr, remote_addr)) = tcp_listener.next().await {
                    debug!("tcp: {},{}", local_addr, remote_addr);

                    let c: ruci::net::Conn = Box::new(stream);

                    let mut new_cid = cc.clone();
                    new_cid.push_num(s_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed));

                    let m = MapResult::new_c(c).new_id(new_cid).build();
                    let r = tx.send(m).await;
                    if let Err(e) = r {
                        warn!(cid = %cc, "stack send tx got error: {}", e);
                        break;
                    }
                }
            });
            debug!(cid = %cid , laddr= self.addr.to_string(), "stack server started");

            match params.shutdown_rx {
                Some(s) => MapResult::builder()
                    .c(ruci::net::Stream::Generator(rx))
                    .a(params.a)
                    .b(params.b)
                    .shutdown_rx(s)
                    .build(),
                None => todo!(),
            }
        } else {
            MapResult::err_str("stack only support None stream")
        }
    }
}

struct AddrConnR {
    base: RecvHalf,
}

impl ruci::Name for AddrConnR {
    fn name(&self) -> &str {
        "lwip_ac_r"
    }
}

struct AddrConnW {
    base: SendHalf,
    src_addr: SocketAddr,
}

impl ruci::Name for AddrConnW {
    fn name(&self) -> &str {
        "lwip_ac_w"
    }
}

impl AsyncReadAddr for AddrConnR {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let f = self.base.recv_from();

        let r = Future::poll(std::pin::pin!(f), cx);

        match r {
            Poll::Ready(r) => match r {
                Ok((data, from, to)) => {
                    debug!("udp: {from},{to}");
                    buf.copy_from_slice(&data);

                    Poll::Ready(Ok((
                        data.len(),
                        ruci::net::Addr {
                            addr: ruci::net::NetAddr::Socket(from),
                            network: ruci::net::Network::UDP,
                        },
                    )))
                }
                Err(e) => Poll::Ready(Err(e)),
            },
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWriteAddr for AddrConnW {
    fn poll_write_addr(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let r = self
            .base
            .send_to(buf, &self.src_addr, &addr.get_socket_addr().unwrap());
        match r {
            Ok(_) => Poll::Ready(io::Result::Ok(buf.len())),
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_flush_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(io::Result::Ok(()))
    }

    fn poll_close_addr(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(io::Result::Ok(()))
    }
}
