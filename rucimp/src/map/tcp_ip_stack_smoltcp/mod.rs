/*!
Defines a [`Map`] called [`Stack`] using user level tcp/ip stack based on `smoltcp`.
 */
pub mod device;
pub mod ip_packet;
pub mod tcp;
pub mod udp;

use std::time::Duration;

use async_trait::async_trait;
use ruci::map::{self, *};
use ruci::net::*;
use ruci::Name;

use macro_map::*;
use smoltcp::iface::PollIngressSingleResult;
use tokio::sync::mpsc;
use tracing::debug;

/// decompose the incomming ip stream into multiple tcp/udp stream.
#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct Stack {}

impl Name for Stack {
    fn name(&self) -> &'static str {
        "smoltcp_stack"
    }
}

#[async_trait]
impl Map for Stack {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::Conn(base_conn) => {
                // base_conn 一般为 tun 设备提供的 Conn, 见 Addr::try_dial 中的 IP 部分

                let mut shutdown_rx = match params.shutdown_rx {
                    Some(r) => r,
                    None => {
                        return MapResult::from_err_str(
                            "smoltcp requires a shutdown_rx for graceful shutdown",
                        )
                    }
                };

                let (new_stream_tx, new_stream_rx) = mpsc::channel(1000);

                tokio::spawn(async move {
                    let device::DeviceAndReceivers {
                        mut iface,
                        mut device,
                        mut tcp_rx,
                        mut udp_rx,
                    } = device::create(cid, base_conn, new_stream_tx);

                    let mut interval = tokio::time::interval(Duration::from_secs(20));

                    loop {
                        tokio::select! {
                            _ = interval.tick() =>{
                                device.udp_health_check();
                            }

                            r = device.read() =>{
                                match r {
                                    Err(e) => {
                                        tracing::warn!("SmoltcpDevice read got e {e}");
                                        break;
                                    },
                                    Ok(_) => {

                                        //poll->socket_ingress->device.receive->rx_token.consume->process_ip->process_ipv4->process_tcp

                                        match device.data.new_read_handle{
                                            device::NewReadType::None => {
                                                let tcp_sockets = &mut device.tcp_sockets as *mut smoltcp::iface::SocketSet;

                                                iface.poll_ingress_single(smoltcp::time::Instant::now(), &mut device, unsafe { &mut *tcp_sockets });
                                                iface.poll_egress(smoltcp::time::Instant::now(), &mut device, unsafe { &mut *tcp_sockets });

                                            },

                                            device::NewReadType::TCP(_) =>{
                                                let tcp_sockets = &mut device.tcp_sockets as *mut smoltcp::iface::SocketSet;

                                                // iface.poll(smoltcp::time::Instant::now(), &mut device, unsafe { &mut *tcp_sockets });
                                                while iface.poll_ingress_single(smoltcp::time::Instant::now(), &mut device, unsafe { &mut *tcp_sockets })
                                                != PollIngressSingleResult::None
                                                {
                                                    // debug!("loop");
                                                    //poll_egress followed by process_ingress is necessary for tcp
                                                    device.process_ingress();
                                                    iface.poll_egress(smoltcp::time::Instant::now(), &mut device, unsafe { &mut *tcp_sockets });

                                                    // debug!("loop e");

                                                }
                                            },
                                            device::NewReadType::UDP(_) =>{

                                                let sockets = &mut device.udp_sockets as *mut smoltcp::iface::SocketSet;

                                                iface.poll_ingress_single(smoltcp::time::Instant::now(),&mut device, unsafe {
                                                    &mut *sockets
                                                });
                                                device.process_ingress();

                                            },
                                        }
                                    },

                                }
                            }
                            r = tcp_rx.recv() =>{
                                match r {
                                    Some((sh,_,b)) => {
                                        device.send_tcp(sh, b);

                                        // egress 之后还是要 poll 一次，否则不会真发出去.

                                        let tcp_sockets = &mut device.tcp_sockets as *mut smoltcp::iface::SocketSet;

                                        iface.poll_egress(smoltcp::time::Instant::now(),&mut device, unsafe {
                                            &mut *tcp_sockets
                                        });

                                    },
                                    None => {
                                        tracing::warn!("SmoltcpDevice tcp_rx read got None");
                                        break;
                                    },
                                }
                            }
                            r = udp_rx.recv() =>{
                                match r {
                                    Some((sh,d,b)) => {
                                        device.send_udp(sh,d, b);

                                        // egress 之后还是要 poll 一次，否则不会真发出去.

                                        let udp_sockets = &mut device.udp_sockets as *mut smoltcp::iface::SocketSet;

                                        iface.poll_egress(smoltcp::time::Instant::now(),&mut device, unsafe {
                                            &mut *udp_sockets
                                        });

                                    },
                                    None => {
                                        tracing::warn!("SmoltcpDevice udp_rx read got None");
                                        break;
                                    },
                                }
                            }
                            _ = &mut shutdown_rx =>{
                                debug!("smoltcp got shutdown signal");
                                break;
                            }
                        } //select!
                    } //loop
                });

                return MapResult::builder()
                    .a(params.a)
                    .b(params.b)
                    .c(ruci::net::Stream::Generator(new_stream_rx))
                    .build();
            }
            _ => {
                return MapResult::from_err_str(&format!(
                    "smoltcp only support Conn stream, got {}",
                    params.c
                ))
            }
        }
    }
}
