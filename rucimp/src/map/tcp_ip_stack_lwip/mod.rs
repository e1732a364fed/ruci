/*
! Implement tcp/ip stack by netstack_lwip;

<https://github.com/eycorsican/netstack-lwip/tree/master>

 */

mod udp;

use std::{
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
use netstack_lwip::NetStack;
use ruci::{map, net::Network};
use ruci::{
    map::{Map, MapParams, MapResult, ProxyBehavior},
    net::{Addr, CID},
    Name,
};
use tokio::{
    io::{split, AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};
use tracing::debug;
use tracing::warn;
use udp::loop_accept_udp;

#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct Stack {}

impl Name for Stack {
    fn name(&self) -> &'static str {
        "lwip_stack"
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
            let (stack, mut tcp_listener, udp_socket) = NetStack::new().unwrap();
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

            let (udp_new_msg_tx_to_lwip, mut udp_new_msg_rx_lwip_end) = mpsc::channel(100);

            let (udp_new_msg_tx_lwip_end, udp_new_msg_rx_self_end) = mpsc::channel(100);

            let cc = cid.clone();
            let ccc = cid.clone();

            let (w, r) = udp_socket.split();

            tokio::spawn(async move {
                loop {
                    let r: Option<(Vec<u8>, SocketAddr, SocketAddr)> =
                        udp_new_msg_rx_lwip_end.recv().await;
                    match r {
                        None => todo!(),

                        Some(d) => {
                            // debug!("will send to stack {},{}", &d.1, &d.2); // 10.0.0.1:55124,114.114.114.114:53
                            // let r = w.send_to(d.0.as_slice(), &d.1, &d.2);
                            let r = w.send_to(d.0.as_slice(), &d.1, &d.2);
                            match r {
                                Ok(_) => {
                                    // debug!("write ok");
                                }
                                Err(_) => todo!(),
                            }
                        }
                    }
                }
            });

            let shutdown_atomic: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
            tokio::spawn(async move {
                loop_accept_udp(r, udp_new_msg_tx_lwip_end, shutdown_atomic).await
            });

            let mut l = udp::Listener::new(udp_new_msg_tx_to_lwip, udp_new_msg_rx_self_end)
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
                        Err(_) => todo!(),
                    }
                }
            });

            tokio::spawn(async move {
                let s_count: AtomicU32 = AtomicU32::new(1);

                while let Some((stream, local_addr, remote_addr)) = tcp_listener.next().await {
                    debug!("lwip new tcp: {},{}", local_addr, remote_addr);

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
                None => todo!(),
            }
        } else {
            MapResult::from_err_str("stack only support None stream")
        }
    }
}
