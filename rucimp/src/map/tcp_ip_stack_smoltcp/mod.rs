/*!
Defines a [`Map`] called [`Stack`] using user level tcp/ip stack based on `smoltcp`.
 */
pub mod device;
pub mod ip_packet;
pub mod tcp;
pub mod udp2;

use std::time::Duration;

use async_trait::async_trait;
use ruci::map::{self, *};
use ruci::net::*;
use ruci::Name;

use macro_map::*;
use tokio::io::AsyncWriteExt;
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
                        return MapResult::err_str(
                            "smoltcp requires a shutdown_rx for graceful shutdown",
                        )
                    }
                };

                let (new_stream_tx, new_stream_rx) = mpsc::channel(1000);

                tokio::spawn(async move {
                    let device::DeviceAndReceivers {
                        mut device,
                        mut w,
                        mut tcp_rx,
                        mut udp_rx,
                        mut device_write_rx,
                    } = device::create(cid, base_conn, new_stream_tx);

                    let mut iface = device::create_interface(&mut device);

                    let mut interval = tokio::time::interval(Duration::from_secs(2));

                    tokio::spawn(async move {
                        loop {
                            let ob = device_write_rx.recv().await;
                            match ob {
                                Some(b) => match w.write(&b).await {
                                    Ok(_) => {}
                                    Err(e) => {
                                        debug!("smoltcp write got e {e}, will break.");
                                        break;
                                    }
                                },
                                None => {
                                    debug!("smoltcp write got None, will break.");
                                    break;
                                }
                            }
                        }
                    });

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
                                        let now = smoltcp::time::Instant::now();

                                        //先给一个 空列表，只利用它调用到 device.receive
                                        // 之后拿到 new_read_type 后，再按实际类型走

                                        let fake_sockets = &mut device.tcp_sockets as *mut smoltcp::iface::SocketSet;

                                        //poll->socket_ingress->device.receive->rx_token.consume->process_ip->process_ipv4->process_tcp

                                        iface.poll(now,&mut device, unsafe {
                                            &mut *fake_sockets
                                        });

                                        match device.new_read_type{
                                            device::NewReadType::None => {},

                                            device::NewReadType::TCP =>{

                                                let tcp_sockets = &mut device.tcp_sockets as *mut smoltcp::iface::SocketSet;

                                                iface.poll(now,&mut device, unsafe {
                                                    &mut *tcp_sockets
                                                });
                                                device.process_ingress();

                                            },
                                            device::NewReadType::UDP =>{
                                                let sockets = &mut device.udp_sockets as *mut smoltcp::iface::SocketSet;

                                                iface.poll(now,&mut device, unsafe {
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
                                        device.process_tcp_egress(sh, b);

                                        // egress 之后还是要 poll 一次，否则不会真发出去.

                                        let tcp_sockets = &mut device.tcp_sockets as *mut smoltcp::iface::SocketSet;

                                        iface.poll(smoltcp::time::Instant::now(),&mut device, unsafe {
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
                                        device.process_udp_egress(sh,d, b);

                                        // egress 之后还是要 poll 一次，否则不会真发出去.

                                        let udp_sockets = &mut device.udp_sockets as *mut smoltcp::iface::SocketSet;

                                        iface.poll(smoltcp::time::Instant::now(),&mut device, unsafe {
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
                return MapResult::err_str(&format!(
                    "smoltcp only support Conn stream, got {}",
                    params.c
                ))
            }
        }
    }
}
