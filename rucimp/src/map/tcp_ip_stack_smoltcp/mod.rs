/*
! Implement tcp/ip stack by netstack_smoltcp;

<https://github.com/automesh-network/netstack-smoltcp>

The mod is a mirror of mod tcp_ip_stack_lwip.

 */

use std::{
    fmt::Display,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicU32},
        Arc,
    },
};

use async_trait::async_trait;
use bytes::BytesMut;
use futures::{SinkExt, StreamExt};
use macro_map::{map_ext_fields, MapExt};
use ruci::{map, net::Network};
use ruci::{
    map::{Map, MapParams, MapResult, ProxyBehavior},
    net::{Addr, CID},
};
use tokio::{
    io::{split, AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};
use tracing::debug;
use tracing::warn;
// use udp::smoltcp_loop_accept_udp;

#[async_trait::async_trait]

impl crate::map::tcp_ip_stack_common::udp::Getter for netstack_smoltcp::udp::ReadHalf {
    async fn get(&mut self) -> std::io::Result<(Vec<u8>, SocketAddr, SocketAddr)> {
        let r = self.next().await;
        r.ok_or(std::io::Error::other("smoltcp udp ReadHalf got None"))
    }
}

pub async fn smoltcp_loop_accept_udp(
    mut r: netstack_smoltcp::udp::ReadHalf,
    tx: mpsc::Sender<crate::map::tcp_ip_stack_common::udp::DataDstSrc>,
    shutdown_atomic: Arc<AtomicBool>,
) {
    crate::map::tcp_ip_stack_common::udp::loop_accept_udp(&mut r, tx, shutdown_atomic).await
}

#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct Stack {}

impl Display for Stack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "smoltcp_stack")
    }
}

#[async_trait]
impl Map for Stack {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        //从 tun device 有三种方式可以 异步读取，
        // 1. 先在 device 用 AsyncDevice 它自己的 split
        // 2. 转为 AsyncConn 后 用 tokio 的 split
        // 3. 转为 Frame 后 分成 sink 和 stream

        //24.12.25: 实测3种情况效果相同。

        // if let ruci::net::Stream::RW(rw) = params.c {
        // if let ruci::net::Stream::Frame(f) = params.c {
        if let ruci::net::Stream::Conn(conn) = params.c {
            // let (stack, mut tcp_listener, udp_socket)
            let (stack, runner, udp_socket, tcp_listener) =
                netstack_smoltcp::StackBuilder::default()
                    .stack_buffer_size(512)
                    .tcp_buffer_size(4096)
                    .enable_udp(true)
                    .enable_tcp(true)
                    .enable_icmp(true)
                    .build()
                    .unwrap();
            let mut tcp_listener = tcp_listener.unwrap();
            let udp_socket = udp_socket.unwrap();
            if let Some(runner) = runner {
                tokio::spawn(runner);
            }

            let (mut stack_sink, mut stack_stream) = stack.split();

            // debug!("try init with {:?},{},{:?}", tun_name, dial_addr, netmask);
            // let r = ruci::net::tun::create_bind_sink_stream(tun_name, dial_addr, netmask).await;
            // 实测使用 frame 转的 stream 和 sink 读取不到任何数据，原因未知，故只能用 原来的 AsyncRead+AsyncWrite 的方式

            // let (mut r, mut w) = rw;
            let (mut r, mut w) = split(conn);

            // let (mut tun_sink, mut tun_stream) = f;

            // Reads packet from TUN and sends to stack.
            tokio::spawn(async move {
                let mut bs = BytesMut::zeroed(4096);
                loop {
                    // debug!("start read bc");
                    let r = r.read(&mut bs).await;
                    if let Ok(n) = r {
                        // debug!("tun got pkt {:?},  {:?}", n, &bs[..n]);
                        stack_sink.send((bs[..n]).to_vec()).await.unwrap();
                    } else {
                        break;
                    }
                }
                // debug!("end2");
            });

            // tokio::spawn(async move {
            //     while let Some(pkt) = tun_stream.next().await {
            //         // debug!("tun got pkt {:?}", pkt);
            //         if let Ok(pkt) = pkt {
            //             stack_sink.send(pkt).await.unwrap();
            //         }
            //     }
            //     debug!("end2");
            // });

            // Reads packet from stack and sends to TUN.
            tokio::spawn(async move {
                while let Some(pkt) = stack_stream.next().await {
                    // debug!("stack got pkt {:?}", pkt);

                    if let Ok(pkt) = pkt {
                        // debug!("stack wrting");
                        w.write_all(&pkt).await.unwrap();
                        // debug!("stack wrting ok");
                    }
                }
                // debug!("end1");
            });

            // tokio::spawn(async move {
            //     while let Some(pkt) = stack_stream.next().await {
            //         // debug!("stack got pkt {:?}", pkt);

            //         if let Ok(pkt) = pkt {
            //             tun_sink.send(pkt).await.unwrap();
            //         }
            //     }
            //     debug!("end1");
            // });

            let (stream_tx, stream_rx) = mpsc::channel(100);

            let stream_tx_c = stream_tx.clone();

            let (udp_new_msg_tx_to_smoltcp, mut udp_new_msg_rx_smoltcp_end) = mpsc::channel(100);

            let (udp_new_msg_tx_smoltcp_end, udp_new_msg_rx_self_end) = mpsc::channel(100);

            let cc = cid.clone();
            let ccc = cid.clone();

            let (r, mut w) = udp_socket.split();

            tokio::spawn(async move {
                loop {
                    let r: Option<(Vec<u8>, SocketAddr, SocketAddr)> =
                        udp_new_msg_rx_smoltcp_end.recv().await;
                    match r {
                        None => {
                            tracing::info!("udp_new_msg_rx_smoltcp_end.recv() got none");
                            break;
                        }

                        Some(d) => {
                            // debug!("will send to stack {},{}", &d.1, &d.2); // 10.0.0.1:55124,114.114.114.114:53
                            // let r = w.send_to(d.0.as_slice(), &d.1, &d.2);
                            use futures::SinkExt;
                            let r = w.send((d.0, d.1, d.2)).await;
                            match r {
                                Ok(_) => {
                                    // debug!("write ok");
                                }
                                Err(e) => {
                                    warn!("w.send_to got err: {e}");
                                    break;
                                }
                            }
                        }
                    }
                }
            });

            let shutdown_atomic: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
            tokio::spawn(async move {
                smoltcp_loop_accept_udp(r, udp_new_msg_tx_smoltcp_end, shutdown_atomic).await
            });

            let mut l = crate::map::tcp_ip_stack_common::udp::Listener::new(
                udp_new_msg_tx_to_smoltcp,
                udp_new_msg_rx_self_end,
            )
            .await
            .unwrap();

            tokio::spawn(async move {
                loop {
                    let r = l.accept().await;
                    match r {
                        Ok(d) => {
                            let m = MapResult::new_u(d.ac)
                                .a(Some(Addr {
                                    addr: ruci::net::NetAddr::Socket(d.dst),
                                    network: Network::UDP,
                                }))
                                .b(Some(d.first_buf))
                                .build();
                            let r = stream_tx_c.send(m).await;
                            if let Err(e) = r {
                                warn!(cid = %ccc, "stack send tx got error: {}", e);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("l.accept() got err {e}");
                            break;
                        }
                    }
                }
            });

            tokio::spawn(async move {
                let s_count: AtomicU32 = AtomicU32::new(1);

                while let Some((stream, local_addr, remote_addr)) = tcp_listener.next().await {
                    debug!("smoltcp new tcp: {},{}", local_addr, remote_addr);

                    let c: ruci::net::Conn = Box::new(stream);

                    let mut new_cid = cc.clone();
                    new_cid.push_num(s_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed));

                    let m = MapResult::new_c(c)
                        .new_id(new_cid)
                        .a(Some(Addr {
                            addr: ruci::net::NetAddr::Socket(remote_addr),
                            network: Network::TCP,
                        }))
                        .build();
                    let r = stream_tx.send(m).await;
                    if let Err(e) = r {
                        warn!(cid = %cc, "stack send tx got error: {}", e);
                        break;
                    }
                }
            });
            debug!(cid = %cid ,   "stack server started");

            match params.shutdown_rx {
                Some(s) => MapResult::builder()
                    .c(ruci::net::Stream::Generator(stream_rx))
                    .a(params.a)
                    .b(params.b)
                    .shutdown_rx(s)
                    .build(),
                None => MapResult::from_err_str("params.shutdown_rx is none"),
            }
        } else {
            MapResult::from_err_str("stack only support None stream")
        }
    }
}
