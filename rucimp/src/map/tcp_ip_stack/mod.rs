/*! Defines common code for tcp/ip stack
 */

#[cfg(feature = "lwip")]
pub mod lwip;

#[cfg(feature = "smoltcp")]
pub mod smoltcp;

pub mod udp;

use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicU32},
        Arc,
    },
};

use bytes::BytesMut;
use futures::{Sink, SinkExt, Stream, StreamExt};

use ruci::net::Network;
use ruci::{
    map::{MapParams, MapResult},
    net::{Addr, CID},
};
use tokio::{
    io::{split, AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};
use tracing::debug;
use tracing::warn;
use udp::{UdpRead, UdpWrite};

#[allow(clippy::type_complexity)]
pub trait Builder: Send + Sync {
    type AsyncConn: ruci::net::AsyncConn + 'static;
    type StackStream: Stream<Item = std::io::Result<Vec<u8>>>
        + Sink<Vec<u8>, Error = std::io::Error>
        + Unpin
        + Send;

    type TcpConnStream: Stream<Item = (Self::AsyncConn, SocketAddr, SocketAddr)> + Unpin + Send;

    fn build(
        &self,
    ) -> (
        Self::StackStream,
        Self::TcpConnStream,
        (Box<dyn UdpRead>, Box<dyn UdpWrite>),
    );
}

pub async fn maps<AsyncConn, TcpConnStream, StackStream>(
    cid: CID,
    params: MapParams,
    builder: &dyn Builder<
        AsyncConn = AsyncConn,
        TcpConnStream = TcpConnStream,
        StackStream = StackStream,
    >,
) -> MapResult
where
    AsyncConn: ruci::net::AsyncConn + 'static,
    TcpConnStream: Stream<Item = (AsyncConn, SocketAddr, SocketAddr)> + Unpin + Send + 'static,
    StackStream: Stream<Item = std::io::Result<Vec<u8>>>
        + Sink<Vec<u8>, Error = std::io::Error>
        + Unpin
        + Send
        + 'static,
{
    //从 tun device 有三种方式可以 异步读取，
    // 1. 先在 device 用 AsyncDevice 它自己的 split
    // 2. 转为 AsyncConn 后 用 tokio 的 split
    // 3. 转为 Frame 后 分成 sink 和 stream

    //24.12.25: 实测3种情况效果相同。

    // if let ruci::net::Stream::RW(rw) = params.c {
    // if let ruci::net::Stream::Frame(f) = params.c {
    if let ruci::net::Stream::Conn(conn) = params.c {
        let (stack, mut tcp_listener, udp_socket) = builder.build();
        let (mut stack_sink, mut stack_stream) = stack.split();

        let (mut r, mut w) = split(conn);

        // Reads packet from TUN and sends to stack.
        tokio::spawn(async move {
            let mut bs = BytesMut::zeroed(4096);
            loop {
                let r = r.read(&mut bs).await;
                if let Ok(n) = r {
                    let r = stack_sink.send((bs[..n]).to_vec()).await;
                    r.unwrap();
                } else {
                    break;
                }
            }
        });

        // Reads packet from stack and sends to TUN.
        tokio::spawn(async move {
            while let Some(pkt) = stack_stream.next().await {
                if let Ok(pkt) = pkt {
                    w.write_all(&pkt).await.unwrap();
                }
            }
        });

        let (stream_tx, stream_rx) = mpsc::channel(100);

        let stream_tx_c = stream_tx.clone();

        let (udp_new_msg_tx_self_end, mut udp_new_msg_rx_stack_end) = mpsc::channel(100);

        let (udp_new_msg_tx_stack_end, udp_new_msg_rx_self_end) = mpsc::channel(100);

        let cc = cid.clone();
        let ccc = cid.clone();

        let (mut udp_r, mut udp_w) = udp_socket;

        tokio::spawn(async move {
            loop {
                let r: Option<(Vec<u8>, SocketAddr, SocketAddr)> =
                    udp_new_msg_rx_stack_end.recv().await;
                match r {
                    None => {
                        tracing::info!("udp_new_msg_rx_stack_end.recv() got none");
                        break;
                    }

                    Some(d) => {
                        let r = udp_w.write((d.0, d.1, d.2)).await;
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
            crate::map::tcp_ip_stack::udp::loop_accept_udp(
                udp_r.as_mut(),
                udp_new_msg_tx_stack_end,
                shutdown_atomic,
            )
            .await
        });

        let mut udp_listener = crate::map::tcp_ip_stack::udp::Listener::new(
            udp_new_msg_tx_self_end,
            udp_new_msg_rx_self_end,
        )
        .await
        .unwrap();

        tokio::spawn(async move {
            loop {
                let r = udp_listener.accept().await;
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
                        tracing::warn!(cid = %ccc,"l.accept() got err {e}");
                        break;
                    }
                }
            }
        });

        tokio::spawn(async move {
            let s_count: AtomicU32 = AtomicU32::new(1);

            while let Some((stream, local_addr, remote_addr)) = tcp_listener.next().await {
                debug!(cid = %cc, "stack new tcp: {},{}", local_addr, remote_addr);

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
