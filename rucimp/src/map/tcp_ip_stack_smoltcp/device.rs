use bytes::{Buf, BytesMut};
use parking_lot::Mutex;
use smoltcp::iface::SocketHandle;
use smoltcp::phy::{Device, RxToken, TxToken};
use tokio::io::{ AsyncReadExt, AsyncWriteExt,  };

use ruci::map::*;
use ruci::net::*;

use smoltcp::socket::tcp;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::{ debug, trace, warn};

use smoltcp::wire::{IpEndpoint, IpProtocol, TcpPacket, UdpPacket};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::Poll;
use anyhow::Context;
const BUF_SIZE: usize = 65535;


/// 通过 SmoltcpDevice 创建一个 smoltcp::iface::Interface, 其在 smoltcp 中是关键
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


/// record data upload and download traffic.
pub struct Traffic {
    //rx_bytes: usize,
    tx_bytes: usize,
    //begin_traffic: std::time::Instant,
}

impl Traffic {
    pub fn new() -> Traffic {
        Self {
            //rx_bytes: 0,
            tx_bytes: 0,
            //begin_traffic: std::time::Instant::now(),
        }
    }
}

/// msg，created by Device's receive method.
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

/// send msg，created by Device's transmit method.
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
        let r = f(&mut self.buf[..len]);
        let _ = futures::executor::block_on( self.conn.write_all(&self.buf[..len]));
        r
    }
}

#[cfg(target_os = "macos")]
const PREFIX: usize = 4;

#[cfg(not(target_os = "macos"))]
const PREFIX: usize = 0;

/// 实现 smoltcp 的 Device trait. 
/// 
/// Device trait 的 receive 方法会在 smoltcp 的 iface.poll 被调用 后自动触发.
/// 
/// 主要 存放的是 sockets, 以及对应的几个 channel 以与 每个 TcpStream 收发信息
pub struct SmoltcpDevice  {
    cid: CID,

    /// base_conn read state
    pub state: Poll<usize>,

    traffic: Traffic,
    buf : Box<[u8; PREFIX + u16::MAX as usize]>,

    /// 在 Device trait 的 transmit 方法被调用后，创建的新 Token 中会有 w 的复本.
    /// 利用它向 base_conn 写入数据
    w:  tokio::io::WriteHalf<ruci::net::Conn>,

    /// for reading the base_conn's data
    r:  tokio::io::ReadHalf<ruci::net::Conn>,


    /// SocketSet 是 smoltcp 提供的socket 的容器, 用 socket 对应的
    /// handle 来提取 某个 socket
    /// 
    /// 在程序运行中，在 iface.poll 之后，我们总是要遍历 所有的 socket，
    /// 并查看每个 socket 的状态, 在可读时用 recv_slice （process_ingress 中），
    /// 在可写时用 send_slice （process_egress 中)
    /// 
    /// 这个 SocketSet 比较坑，没有check 方法，一旦get/remove 时 不存在就会 panic
    pub sockets: smoltcp::iface::SocketSet<'static>,


    tcp_src_handle_map: Arc<Mutex<HashMap<IpEndpoint, SocketHandle>>>,
    udp_src_handle_map: Arc<Mutex<HashMap<SocketAddr, SocketHandle>>>,


    /// 生成新 Stream 后由此发出.
    new_stream_tx: tokio::sync::mpsc::Sender<MapResult>,

    /// write the data read from smoltcp to tx, whose rx is inside TcpStream to be read
    tcp_read_data_tx_map: Arc<Mutex<HashMap<IpEndpoint, Sender<BytesMut>>>>,

    /// only here to be cloned for new TcpStream
    tcp_write_data_tx: Sender<(SocketHandle, SocketAddr, BytesMut)>,

    /// receive tcp new write data from all the TcpStream
    tcp_write_data_rx: Receiver<(SocketHandle,SocketAddr, BytesMut)>,

}



impl  Device for SmoltcpDevice  {
    type RxToken<'a> = MyRxToken<'a> 
    where
        Self: 'a;

    type TxToken<'a> = MyTxToken<'a> 
    where
        Self: 'a;

    /// only proceed when self.state == Poll::Ready(n), and will
    /// set it back to Poll::Pending immediately.
    fn receive(
        &mut self,
        _timestamp: smoltcp::time::Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {

        match self.state {
            Poll::Pending => return None,
            Poll::Ready(n) => {
                self.check_read_buf_for_new_conn(n);
                let data = &mut self.buf[PREFIX..n];

                let rx = MyRxToken { data };
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
        debug!("transmit called");
        Some(MyTxToken{ conn: &mut self.w, traffic: &mut self.traffic, buf: [0u8;BUF_SIZE] })
    }

    fn capabilities(&self) -> smoltcp::phy::DeviceCapabilities {
        let mut dc = smoltcp::phy::DeviceCapabilities::default();
        dc.medium = smoltcp::phy::Medium::Ip;
        dc.max_transmission_unit = 1500;
        dc
    }
}

/// 判断一个TcpPacket 是否为一个新的Tcp Stream 的第一条信息
#[inline]
fn is_tcp_client_hello(tcp_packet: &TcpPacket<&[u8]>) -> bool {
    tcp_packet.syn() && !tcp_packet.ack()
}

 
 
impl  SmoltcpDevice {

    /// 接受的 net_stream_tx 将被用于向外发送 从 base_conn 新解析出的 tcp/udp stream.
    pub fn new(  cid: CID,  base_conn: ruci::net::Conn,new_stream_tx: tokio::sync::mpsc::Sender<MapResult>,)->Self{
        let (r,w ) = tokio::io::split(base_conn);

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

    /// read the base_conn's ReadHalf part(`r`), data will be written in `buf`。
    /// 
    /// On a successful read, self.state will be set to Poll::Ready(n), with n the 
    /// data length.
     pub async fn read(&mut self)->anyhow::Result<()>{
        let n = self.r.read(self.buf.as_mut()).await?;
        if n < PREFIX{
            anyhow::bail!("smoltcp_stack read got n less than: {PREFIX} {n}");
        }else{
        debug!("smoltcp device read {n}");

            self.state = Poll::Ready(n)
        }

        Ok(())
    }


    /// 被 Device trait 的 receive 方法调用, 检查 self.buf, 判断是否有新 tcp 产生，如有, 建立新 TcpStream 并 送入 new_stream_tx, 并创建新的 sockethandle 放入 sockets，
    fn check_read_buf_for_new_conn(&mut self,
        n:usize,
    ) {
        let data = & self.buf[0..n];//mac 上不能为 PREFIX, 而要为0

        debug!("check_read_buf_for_new_conn {n}");


        let packet =
            super::ip_packet::IpPacket::new_checked(data).context("convert frame to IpPacket failed");
        let ip_packet = match packet {
            Ok(p) => p,
            Err(e) => {
                warn!(cid = %self.cid, "smoltcp check_read_buf_for_new_conn got err: {e}");
                return;
            }
        };
        let src_ip_addr = ip_packet.src_addr();
        let dst_ip_addr = ip_packet.dst_addr();

        match ip_packet.protocol() {
            IpProtocol::Icmp | IpProtocol::Icmpv6 => {
                debug!("is icmp, {n} {}",ip_packet.payload().len())
            },
            IpProtocol::Tcp => {
                debug!("is tcp, {n} {}",ip_packet.payload().len());
                let tcp_packet = match TcpPacket::new_checked(ip_packet.payload()) {
                    Ok(p) => p,
                    Err(err) => {
                        warn!(cid = %self.cid,
                            "smoltcp check_read_buf_for_new_conn got invalid TCP packet err: {}, src_ip: {}, dst_ip: {}, payload: {}",
                            err,
                            src_ip_addr,
                            dst_ip_addr,
                            ruci::utils::HexSlice(ip_packet.payload())
                        );
                        return;
                    }
                };

                let src_port = tcp_packet.src_port();
                let dst_port = tcp_packet.dst_port();

                let src_addr = SocketAddr::new(src_ip_addr, src_port);
                let dst_addr = SocketAddr::new(dst_ip_addr, dst_port);

                let ipe = src_addr.into();

                let is_hello = is_tcp_client_hello(&tcp_packet);
                let contains = self.tcp_src_handle_map.lock().contains_key(&ipe);

                if is_hello && !contains{
                    
                    let rx_buffer = tcp::SocketBuffer::new(vec![0; BUF_SIZE]);
                    let tx_buffer = tcp::SocketBuffer::new(vec![0; BUF_SIZE]);
                    let mut new_tcp_socket = tcp::Socket::new(rx_buffer, tx_buffer);
                    new_tcp_socket.set_timeout(Some(smoltcp::time::Duration::from_secs(7200)));
                    if let Err(err) = new_tcp_socket.listen(dst_addr) {
                        warn!("listen error: {:?}", err);
                        return;
                    }
                    // new_tcp_socket
                    // .connect(iface.context(), dst_addr, src_addr)
                    // .unwrap();

                    new_tcp_socket.set_nagle_enabled(false);
                    new_tcp_socket.set_ack_delay(None);

                    let socket_handle = self.sockets.add(new_tcp_socket);
                    self.tcp_src_handle_map.lock().insert(ipe, socket_handle);

                    let (read_tx, read_rx) = tokio::sync::mpsc::channel(100);
                    self.tcp_read_data_tx_map.lock().insert(ipe, read_tx);
                    let tcp_stream = super::tcp::TcpStream::new(
                        read_rx,
                        self.tcp_write_data_tx.clone(),
                        src_addr,
                        dst_addr,
                        socket_handle,
                    );


                    let ta = Addr{addr: NetAddr::Socket(dst_addr),network: Network::TCP};
                    let sa = Addr{addr: NetAddr::Socket(src_addr),network: Network::TCP};

                    debug!("smoltcp got new tcp connection {ta} {sa}");


                    let _ = self.new_stream_tx.try_send(MapResult::new_c(Box::new(tcp_stream)).a(Some(ta)).build());

                }else{
                    debug!("smoltcp got other tcp {is_hello} {contains}");

                }
            }
            IpProtocol::Udp => {
                debug!("is udp, {n} {}",ip_packet.payload().len());
                let packet = UdpPacket::new_checked(ip_packet.payload()).unwrap();
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

                    //let ac = super::udp::new(socket, None);
                }
            }

            _ => {
                trace!("got unhandled protocol {}", ip_packet.protocol())
            }
        }
    }


    /// 名称跟随 smoltcp 的规范.  从smoltcp 对每个 socket 用 recv_slice 读取数据
    pub fn process_ingress(&mut self) {
        let mut handles_to_remove = Vec::new();
        let mut tcp_src_to_remove = Vec::new();

        debug!("process_ingress...");
        

        self.sockets.iter_mut().for_each(|(h,so)|{
            match so {
                smoltcp::socket::Socket::Icmp(_) => {},
                smoltcp::socket::Socket::Udp(_) => {},
                smoltcp::socket::Socket::Tcp(so) => {
                    debug!("process_ingress1...");

                    if !so.can_recv() {
                        debug!("process_ingress...1");

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
                    let tcp_read_data_sender = m.get(&src).unwrap();

                    debug!("process_ingress...2");

                    while so.can_recv() && tcp_read_data_sender.capacity() > 0 {
                        let mut buffer = BytesMut::with_capacity(so.recv_queue());
                        unsafe {
                            buffer.set_len(so.recv_queue());
                        }
                        if let Ok(n) = so.recv_slice(buffer.as_mut()) {

                            debug!("process_ingress...3");

                            if n != buffer.len() {
                                tracing::warn!("so.recv_slice n != buffer.len(), {n} {}",buffer.len());
                                unsafe {
                                    buffer.set_len(n);
                                }
                            }
                            let r = tcp_read_data_sender.try_send(buffer);
                            if let Err(e)=r {
                                tracing::warn!("tcp_read_data_tx send failed, {e}");
                                so.close();
                                break;
                            }
                        } else {

                            debug!("process_ingress...4");

                            tracing::debug!("tcp_read_data_tx so.recv_slice failed");
                            so.close();
                            break;
                        }
                    }
                    if so.state() == smoltcp::socket::tcp::State::CloseWait && so.send_queue() == 0 {
                        let _ = tcp_read_data_sender.try_send(BytesMut::with_capacity(0));
                        so.close();
                    }
                    if !so.is_active() {
                        tcp_src_to_remove.push(src);
                    }
                },
            }//match
        });//iter

        for endpoint in tcp_src_to_remove {
            self.remove_tcp(endpoint);
        }
        // for endpoint in udp_endpoints {
        //    // self.remove_udp(endpoint);
        // }
        for handle in handles_to_remove {
            self.sockets.remove(handle);
        }
    }

    /// 名称跟随 smoltcp 的规范. 从tcp/udp 的我们自建的缓存中 对每个socket 读取要写的数据，并用 send_slice 写入 smoltcp
    pub fn process_egress(&mut self) {
        while let Ok((sh, _source, mut data)) = self.tcp_write_data_rx.try_recv() {

            debug!("process_egress...");

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

    fn remove_tcp(&mut self, src: IpEndpoint) {
        tracing::info!("remove tcp {}", src);
        let mut tcp_src_handle_map_lock = self.tcp_src_handle_map.lock();
        if let Some(h) = tcp_src_handle_map_lock.get(&src) {
            self.sockets.remove(*h);
            tcp_src_handle_map_lock.remove(&src);
        }
    }
 
}
