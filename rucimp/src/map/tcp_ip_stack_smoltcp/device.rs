use bytes::{Buf, BytesMut};
use parking_lot::Mutex;
use smoltcp::iface::SocketHandle;
use smoltcp::phy::{Device, RxToken, TxToken};
use tokio::io::{ AsyncReadExt, AsyncWriteExt,  };

const BUF_SIZE: usize = 65535;


pub fn create_interface(device: &mut SmoltcpDevice) -> smoltcp::iface::Interface {
    use smoltcp::wire;

    let mut config = smoltcp::iface::Config::new(wire::HardwareAddress::Ip);
    config.random_seed = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 这里需要 device 只是因为要它的 capabilities
    let mut interface =
        smoltcp::iface::Interface::new(config, device, smoltcp::time::Instant::now());
    interface.set_any_ip(true);
    interface
        .routes_mut()
        .add_default_ipv4_route(wire::Ipv4Address::new(0, 0, 0, 1))
        .unwrap();

    interface.update_ip_addrs(|ips| {
        ips.push(wire::IpCidr::new(
            wire::IpAddress::Ipv4(wire::Ipv4Address::new(0, 0, 0, 1)),
            32,
        ))
        .unwrap();
    });
    interface
}



pub struct Traffic {
    rx_bytes: usize,
    tx_bytes: usize,
    begin_traffic: std::time::Instant,
}

impl Traffic {
    pub fn new() -> Traffic {
        Self {
            rx_bytes: 0,
            tx_bytes: 0,
            begin_traffic: std::time::Instant::now(),
        }
    }
}

pub struct MyRxToken<'a> {
    data: &'a mut [u8],
}
impl<'a> RxToken for MyRxToken<'a> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        f(self.data)
    }
}

/// send msg
pub struct MyTxToken<'a> {
    conn: &'a mut tokio::io::WriteHalf<ruci::net::Conn>,
    traffic: &'a mut Traffic,
    buf: [u8;BUF_SIZE]
}
impl<'a> TxToken for MyTxToken<'a> {
    fn consume<R, F>(mut self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        self.traffic.tx_bytes += len;
        let r = f(&mut self.buf);
        let _ = self.conn.write_all(&self.buf[..len]);

        r
    }
}

#[cfg(target_os = "macos")]
const PREFIX: usize = 4;

#[cfg(not(target_os = "macos"))]
const PREFIX: usize = 0;


pub struct SmoltcpDevice  {
    pub state: Poll<usize>,

    cid: CID,
    traffic: Traffic,
    buf : Box<[u8; PREFIX + u16::MAX as usize]>,
    w:  tokio::io::WriteHalf<ruci::net::Conn>,
    r:  tokio::io::ReadHalf<ruci::net::Conn>,


    pub sockets: smoltcp::iface::SocketSet<'static>,

    new_stream_tx: tokio::sync::mpsc::Sender<MapResult>,

    /// write the data read from smoltcp to tx, whose rx is inside TcpStream to be read
    tcp_read_data_tx_map: Arc<Mutex<HashMap<IpEndpoint, Sender<BytesMut>>>>,

    tcp_src_handle_map: Arc<Mutex<HashMap<SocketAddr, SocketHandle>>>,
    udp_src_handle_map: Arc<Mutex<HashMap<SocketAddr, SocketHandle>>>,

    tcp_write_data_tx: Sender<(SocketHandle, SocketAddr, BytesMut)>,
    tcp_write_data_rx: Receiver<(SocketHandle,SocketAddr, BytesMut)>,

}

 
impl  SmoltcpDevice {
    pub fn new(  cid: CID,  conn: ruci::net::Conn,new_stream_tx: tokio::sync::mpsc::Sender<MapResult>,)->Self{
        let (r,w ) = tokio::io::split(conn);

        let (tcp_write_data_tx, tcp_write_data_rx) = mpsc::channel(100);
       // let (udp_sender, udp_receiver) = mpsc::channel(100);

        Self { 
            cid, traffic: Traffic::new(), buf: Box::new([0; PREFIX + u16::MAX as usize]), w, r, 
            sockets: smoltcp::iface::SocketSet::new([])        ,
             new_stream_tx, 
             tcp_read_data_tx_map: Arc::new(Mutex::new(HashMap::new())), 
             tcp_src_handle_map:  Arc::new(Mutex::new(HashMap::new())), 
              udp_src_handle_map: Arc::new(Mutex::new(HashMap::new())), 
              tcp_write_data_tx,
            tcp_write_data_rx, 
            state: Poll::Pending
        }
    }

     pub async fn read(&mut self)->anyhow::Result<()>{
        let n = self.r.read(self.buf.as_mut()).await?;
        if n < PREFIX{
            anyhow::bail!("smoltcp_stack read got n less than: {PREFIX} {n}");
        }else{
            self.state = Poll::Ready(n)
        }

        Ok(())
    }

    /// 名称跟随 smoltcp 的规范. 意思是从smoltcp 读取数据并处理
    fn process_ingress(&mut self) {
        let mut handles_to_remove = Vec::new();
        let mut tcp_src_to_remove = Vec::new();
        

        self.sockets.iter_mut().for_each(|(h,so)|{
            match so {
                smoltcp::socket::Socket::Icmp(_) => {},
                smoltcp::socket::Socket::Udp(_) => {},
                smoltcp::socket::Socket::Tcp(so) => {
                    if !so.can_recv() {
                        return;
                    }
                    let src = match so.remote_endpoint(){
                        Some(s) => s,
                        None => {
                            handles_to_remove.push(h);
                            return;
                        },
                    };
                    let m = self.tcp_read_data_tx_map.lock();
                    let tx = m.get(&src).unwrap();
                    while so.can_recv() && tx.capacity() > 0 {
                        let mut buffer = BytesMut::with_capacity(so.recv_queue());
                        unsafe {
                            buffer.set_len(so.recv_queue());
                        }
                        if let Ok(n) = so.recv_slice(buffer.as_mut()) {
                            if n != buffer.len() {
                                tracing::warn!("so.recv_slice n != buffer.len(), {n} {}",buffer.len());
                                unsafe {
                                    buffer.set_len(n);
                                }
                            }
                            let r = tx.try_send(buffer);
                            if let Err(e)=r {
                                tracing::warn!("tcp_read_data_tx send failed, {e}");
                                so.close();
                                break;
                            }
                        } else {
                            so.close();
                            break;
                        }
                    }
                    if so.state() == smoltcp::socket::tcp::State::CloseWait && so.send_queue() == 0 {
                        let _ = tx.try_send(BytesMut::with_capacity(0));
                        so.close();
                    }
                    if !so.is_active() {
                        tcp_src_to_remove.push(src);
                    }
                },
            }
        });

        for endpoint in tcp_src_to_remove {
           // self.remove_tcp(endpoint);
        }
        // for endpoint in udp_endpoints {
        //    // self.remove_udp(endpoint);
        // }
        for handle in handles_to_remove {
           // self.remove_tcp_handle(handle);
        }
    }

    /// 名称跟随 smoltcp 的规范. 意思是从tcp/udp socket 读取要写的数据，并写入 smoltcp
    fn process_egress(&mut self) {
        while let Ok((sh, source, mut data)) = self.tcp_write_data_rx.try_recv() {
            let socket: &mut smoltcp::socket::tcp::Socket = self.sockets.get_mut(sh);

             if data.is_empty() {
                socket.close();
                 continue;
             }
             let mut left_data = data.len();

             while left_data>0 {

                let r = socket.send_slice(&data);

                match r {
                   Ok(n) => {
                        left_data -= n;
                        data.advance(n);
                   },
                   Err(_) => {socket.close();continue},
                }
             }

         

        }
    }
}


impl  Device for SmoltcpDevice  {
    type RxToken<'a> = MyRxToken<'a> 
    where
        Self: 'a;

    type TxToken<'a> = MyTxToken<'a> 
    where
        Self: 'a;

    fn receive(
        &mut self,
        _timestamp: smoltcp::time::Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {

        match self.state {
            Poll::Pending => return None,
            Poll::Ready(n) => {
                self.check_frame_for_new_conn(n);
                let frame = &mut self.buf[PREFIX..n];

                let rx = MyRxToken { data:frame };
                let tx = MyTxToken {
                    conn: &mut self.w,
                    traffic: &mut self.traffic,
                    buf: [0u8;BUF_SIZE] 
                };
                self.state = Poll::Pending;
                Some((rx, tx))
            },
        }
    }

    fn transmit(&mut self, _timestamp: smoltcp::time::Instant) -> Option<Self::TxToken<'_>> {
        Some(MyTxToken{ conn: &mut self.w, traffic: &mut self.traffic, buf: [0u8;BUF_SIZE] })
    }

    fn capabilities(&self) -> smoltcp::phy::DeviceCapabilities {
        let mut dc = smoltcp::phy::DeviceCapabilities::default();
        dc.medium = smoltcp::phy::Medium::Ip;
        dc.max_transmission_unit = 1500;
        dc
    }
}

#[inline]
pub fn is_tcp_client_hello(tcp_packet: &TcpPacket<&[u8]>) -> bool {
    tcp_packet.syn() && !tcp_packet.ack()
}


use ruci::map::*;
use ruci::net::*;

use smoltcp::socket::tcp;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::{ trace, warn};

use smoltcp::wire::{IpEndpoint, IpProtocol, TcpPacket, UdpPacket};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::Poll;
use anyhow::Context;

impl SmoltcpDevice {
    
fn check_frame_for_new_conn(&mut self,
    n:usize,
) {
    let frame = & self.buf[PREFIX..n];


    let packet =
        super::ip_packet::IpPacket::new_checked(frame).context("convert frame to IpPacket failed");
    let packet = match packet {
        Ok(p) => p,
        Err(e) => {
            warn!(cid = %self.cid, "smoltcp handle_tun_frame got err: {e}");
            return;
        }
    };
    let src_ip_addr = packet.src_addr();
    let dst_ip_addr = packet.dst_addr();

    match packet.protocol() {
        IpProtocol::Icmp | IpProtocol::Icmpv6 => {},
        IpProtocol::Tcp => {
            let tcp_packet = match TcpPacket::new_checked(packet.payload()) {
                Ok(p) => p,
                Err(err) => {
                    warn!(cid = %self.cid,
                        "smoltcp handle_tun_frame got invalid TCP packet err: {}, src_ip: {}, dst_ip: {}, payload: {}",
                        err,
                        src_ip_addr,
                        dst_ip_addr,
                        ruci::utils::HexSlice(packet.payload())
                    );
                    return;
                }
            };

            let src_port = tcp_packet.src_port();
            let dst_port = tcp_packet.dst_port();

            let src_addr = SocketAddr::new(src_ip_addr, src_port);
            let dst_addr = SocketAddr::new(dst_ip_addr, dst_port);

            if is_tcp_client_hello(&tcp_packet) && !self.tcp_src_handle_map.lock().contains_key(&src_addr){
                 
                let rx_buffer = tcp::SocketBuffer::new(vec![0; BUF_SIZE]);
                let tx_buffer = tcp::SocketBuffer::new(vec![0; BUF_SIZE]);
                let mut new_tcp_socket = tcp::Socket::new(rx_buffer, tx_buffer);
                new_tcp_socket.set_timeout(Some(smoltcp::time::Duration::from_secs(7200)));
                if let Err(err) = new_tcp_socket.listen(dst_addr) {
                    warn!("listen error: {:?}", err);
                    return;
                }

                let sh = self.sockets.add(new_tcp_socket);
                self.tcp_src_handle_map.lock().insert(src_addr, sh);

                let (read_tx, read_rx) = tokio::sync::mpsc::channel(100);
                self.tcp_read_data_tx_map.lock().insert(src_addr.into(), read_tx);
                let c = super::tcp::TcpStream::new(
                    read_rx,
                    self.tcp_write_data_tx.clone(),
                    src_addr,
                    dst_addr,
                    sh,
                );

                let _ = self.new_stream_tx.try_send(MapResult::new_c(Box::new(c)).build());

            }
        }
        IpProtocol::Udp => {
            let packet = UdpPacket::new_checked(packet.payload()).unwrap();
            let src_port = packet.src_port();
            let dst_port = packet.dst_port();
            let src_addr = SocketAddr::new(src_ip_addr, src_port);
            let dst_addr = SocketAddr::new(dst_ip_addr, dst_port);

            if !self.udp_src_handle_map.lock().contains_key(&src_addr){
                use smoltcp::socket::udp::PacketBuffer;
                use smoltcp::socket::udp::PacketMetadata;

                let mut socket = smoltcp::socket::udp::Socket::new(
                    PacketBuffer::new(
                        vec![PacketMetadata::EMPTY; BUF_SIZE / 1500],
                        vec![0; BUF_SIZE],
                    ),
                    PacketBuffer::new(
                        vec![PacketMetadata::EMPTY; BUF_SIZE / 1500],
                        vec![0; BUF_SIZE],
                    ),
                );
                socket.bind(dst_addr).unwrap();
                // let sh = self.sockets.add(socket);
                // self.udp_src_handle_map.lock().insert(src_addr, sh);

                let ac = super::udp::new(socket, None);
            }
        }

        _ => {
            trace!("got unhandled protocol {}", packet.protocol())
        }
    }
}

}
