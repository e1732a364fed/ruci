/*!
Defines the [`SmoltcpDevice`] which is a [`Device`] required in [`smoltcp::iface::Interface`].
 */

use bytes::{Buf, BytesMut};
use parking_lot::Mutex;
use smoltcp::iface::SocketHandle;
use smoltcp::phy::{Device, RxToken, TxToken};
use tokio::io::AsyncReadExt;

use ruci::map::*;
use ruci::net::*;

use smoltcp::socket::tcp;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::{debug, warn};

use anyhow::Context;
use smoltcp::wire::{IpEndpoint, IpProtocol, TcpPacket, UdpPacket};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::Poll;

use super::ip_packet::IpPacket;

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

/*

/// record data upload and download traffic.
pub struct Traffic {
    //rx_bytes: usize,
    tx_bytes: usize,
    //begin_traffic: std::time::Instant,
}

impl Default for Traffic {
    fn default() -> Self {
        Self::new()
    }
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
 */

/// to record a received message，created by Device's receive method.
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

/// send msg，created by Device's transmit (and receive) method.
pub struct MyTxToken {
    tx: Sender<BytesMut>,
    //traffic: &'a mut Traffic,
    //buf: [u8; BUF_SIZE],
}
impl TxToken for MyTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf = BytesMut::zeroed(len);
        //self.traffic.tx_bytes += len;
        let r = f(&mut buf);
        let _ = self.tx.try_send(buf);

        r
    }
}

/// 实现 smoltcp 的 Device trait.
///
/// Device trait 的 receive 方法会在 smoltcp 的 iface.poll 被调用 后自动触发.
///
/// 主要 存放的是 sockets, 以及对应的几个 channel 以与 每个 TcpStream 收发信息
pub struct SmoltcpDevice {
    cid: CID,

    //base_conn: Conn,
    r: tokio::io::ReadHalf<Conn>,

    /// base_conn read state
    r_state: Poll<usize>,
    rbuf: Box<[u8; u16::MAX as usize]>,

    device_write_tx: Sender<BytesMut>,

    /// 生成新 Stream 后由此发出.
    new_stream_tx: tokio::sync::mpsc::Sender<MapResult>,

    /// SocketSet 是 smoltcp 提供的socket 的容器, 用 socket 对应的
    /// handle 来提取 某个 socket
    ///
    /// 在程序运行中，在 iface.poll 之后，我们总是要遍历 所有的 socket，
    /// 并查看每个 socket 的状态, 在可读时用 recv_slice （process_ingress 中），
    /// 在可写时用 send_slice （process_egress 中)
    ///
    /// 这个 SocketSet 比较坑，没有check 方法: 一旦get/remove 时 不存在, 就会 panic
    pub sockets: smoltcp::iface::SocketSet<'static>,

    tcp_src_handle_map: Arc<Mutex<HashMap<IpEndpoint, SocketHandle>>>,
    udp_src_handle_map: Arc<Mutex<HashMap<IpEndpoint, SocketHandle>>>,

    /// write the data read from smoltcp to tx, whose rx is inside TcpStream to be read
    tcp_read_data_tx_map: Arc<Mutex<HashMap<IpEndpoint, Sender<BytesMut>>>>,

    udp_read_data_tx_map: Arc<Mutex<HashMap<IpEndpoint, Sender<(IpEndpoint, BytesMut)>>>>,

    /// only here to be cloned for new TcpStream
    tcp_write_data_tx: Sender<(SocketHandle, SocketAddr, BytesMut)>,

    /// only here to be cloned for new UdpStream
    udp_write_data_tx: Sender<(SocketHandle, IpEndpoint, BytesMut)>,
    // 用于流量记录
    //traffic: Traffic,
}

impl Device for SmoltcpDevice {
    type RxToken<'a> = MyRxToken<'a>
    where
        Self: 'a;

    type TxToken<'a> = MyTxToken
    where
        Self: 'a;

    /// called by smoltcp's iface.poll -> socket.ingress.
    ///
    /// only proceed when self.state == Poll::Ready(n), and will
    /// set it back to Poll::Pending immediately.
    fn receive(
        &mut self,
        _timestamp: smoltcp::time::Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        match self.r_state {
            Poll::Pending => None,
            Poll::Ready(n) => {
                self.check_read_buf_for_new_conn(n);
                let data = &mut self.rbuf[..n];

                let rx = MyRxToken { data };
                let tx = MyTxToken {
                    tx: self.device_write_tx.clone(),
                    //traffic: &mut self.traffic,
                    //buf: [0u8; BUF_SIZE],
                };
                self.r_state = Poll::Pending;
                Some((rx, tx))
            }
        }
    }

    fn transmit(&mut self, _timestamp: smoltcp::time::Instant) -> Option<Self::TxToken<'_>> {
        Some(MyTxToken {
            tx: self.device_write_tx.clone(),
            //traffic: &mut self.traffic,
            //buf: [0u8; BUF_SIZE],
        })
    }

    fn capabilities(&self) -> smoltcp::phy::DeviceCapabilities {
        let mut dc = smoltcp::phy::DeviceCapabilities::default();
        dc.medium = smoltcp::phy::Medium::Ip;
        dc.max_transmission_unit = MTU;
        dc
    }
}

/// returned by SmoltcpDevice::create
pub struct DeviceAndReceivers {
    pub device: SmoltcpDevice,
    pub w: tokio::io::WriteHalf<Conn>,
    pub tcp_rx: Receiver<(SocketHandle, SocketAddr, BytesMut)>,
    pub udp_rx: Receiver<(SocketHandle, IpEndpoint, BytesMut)>,
    pub device_write_rx: Receiver<BytesMut>,
}

/// 返回 SmoltcpDevice，和 接收 tcp 写新信息 的 Receiver 和 接收 udp 写新信息 的 Receiver
///
///  接受的 net_stream_tx 将被用于向外发送 从 base_conn 新解析出的 tcp/udp stream.
pub fn create(
    cid: CID,
    base_conn: ruci::net::Conn,
    new_stream_tx: tokio::sync::mpsc::Sender<MapResult>,
) -> DeviceAndReceivers {
    let (device_write_tx, device_write_rx) = mpsc::channel(10000);

    let (tcp_write_data_tx, tcp_write_data_rx) = mpsc::channel(100);
    let (udp_write_data_tx, udp_write_data_rx) = mpsc::channel(100);

    let (r, w) = tokio::io::split(base_conn);

    let device = SmoltcpDevice {
        // base_conn,
        r,
        cid,
        //traffic: Traffic::new(),
        device_write_tx,

        rbuf: Box::new([0; u16::MAX as usize]),
        r_state: Poll::Pending,

        new_stream_tx,

        sockets: smoltcp::iface::SocketSet::new([]),

        tcp_read_data_tx_map: Arc::new(Mutex::new(HashMap::new())),
        udp_read_data_tx_map: Arc::new(Mutex::new(HashMap::new())),

        tcp_src_handle_map: Arc::new(Mutex::new(HashMap::new())),
        udp_src_handle_map: Arc::new(Mutex::new(HashMap::new())),
        tcp_write_data_tx,
        udp_write_data_tx,
    };

    DeviceAndReceivers {
        device,
        w,
        tcp_rx: tcp_write_data_rx,
        udp_rx: udp_write_data_rx,
        device_write_rx,
    }
}

impl SmoltcpDevice {
    /// read the base_conn, data will be written in `buf`。
    ///
    /// On a successful read, self.state will be set to Poll::Ready(n), with n the
    /// data length.
    ///
    /// `state` will be checked by the `receive` method.
    pub async fn read(&mut self) -> anyhow::Result<()> {
        let n = self.r.read(self.rbuf.as_mut()).await?;

        //debug!("smoltcp device read {n}");

        self.r_state = Poll::Ready(n);

        Ok(())
    }

    // pub async fn write(&mut self, b: BytesMut) -> anyhow::Result<()> {
    //     self.base_conn.write_all(&b).await?;
    //     Ok(())
    // }

    /// 被 Device trait 的 receive 方法调用, 检查 self.buf, 判断是否有新 tcp 产生，如有, 建立新 TcpStream 并 送入 new_stream_tx, 并创建新的 sockethandle 放入 sockets，
    fn check_read_buf_for_new_conn(&mut self, n: usize) {
        //debug!("check_read_buf_for_new_conn {n}");

        let packet =
            IpPacket::new_checked(&self.rbuf[..n]).context("convert frame to IpPacket failed");
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
                //debug!("is icmp, {n} {}",ip_packet.payload().len())
            }
            IpProtocol::Tcp => {
                //debug!("is tcp, {n} {}",ip_packet.payload().len());
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

                /// 判断一个TcpPacket 是否为一个新的Tcp Stream 的第一条信息
                #[inline]
                fn is_tcp_client_hello(tcp_packet: &TcpPacket<&[u8]>) -> bool {
                    tcp_packet.syn() && !tcp_packet.ack()
                }

                if is_tcp_client_hello(&tcp_packet) {
                    let src_port = tcp_packet.src_port();
                    let dst_port = tcp_packet.dst_port();

                    let src_addr = SocketAddr::new(src_ip_addr, src_port);
                    let dst_addr = SocketAddr::new(dst_ip_addr, dst_port);

                    let ipe = src_addr.into();

                    if let Entry::Vacant(e) = self.tcp_src_handle_map.lock().entry(ipe) {
                        let rx_buffer = tcp::SocketBuffer::new(vec![0; BUF_SIZE]);
                        let tx_buffer = tcp::SocketBuffer::new(vec![0; BUF_SIZE]);
                        let mut new_tcp_socket = tcp::Socket::new(rx_buffer, tx_buffer);
                        new_tcp_socket.set_timeout(Some(smoltcp::time::Duration::from_secs(7200)));
                        if let Err(err) = new_tcp_socket.listen(dst_addr) {
                            warn!("listen error: {:?}", err);
                            return;
                        }

                        new_tcp_socket.set_nagle_enabled(false);
                        new_tcp_socket.set_ack_delay(None);

                        let socket_handle = self.sockets.add(new_tcp_socket);
                        e.insert(socket_handle);

                        let (read_tx, read_rx) = tokio::sync::mpsc::channel(100);
                        self.tcp_read_data_tx_map.lock().insert(ipe, read_tx);
                        let tcp_stream = super::tcp::TcpStream::new(
                            read_rx,
                            self.tcp_write_data_tx.clone(),
                            src_addr,
                            dst_addr,
                            socket_handle,
                        );

                        let ta = Addr {
                            addr: NetAddr::Socket(dst_addr),
                            network: Network::TCP,
                        };

                        //debug!("smoltcp got new tcp connection {ta} {src_addr}");

                        let _ = self
                            .new_stream_tx
                            .try_send(MapResult::new_c(Box::new(tcp_stream)).a(Some(ta)).build());
                    }
                }
            }
            IpProtocol::Udp => {
                //debug!("is udp, {n} {}",ip_packet.payload().len());

                let packet = UdpPacket::new_checked(ip_packet.payload()).unwrap();
                let src_port = packet.src_port();

                let src_addr = SocketAddr::new(src_ip_addr, src_port);

                //let dst_ipe: IpEndpoint = dst_addr.into();
                let src_ipe = src_addr.into();

                if let Entry::Vacant(e) = self.udp_src_handle_map.lock().entry(src_ipe) {
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

                    let dst_port = packet.dst_port();
                    let dst_addr = SocketAddr::new(dst_ip_addr, dst_port);

                    socket.bind(dst_addr).unwrap();
                    let sh = self.sockets.add(socket);
                    e.insert(sh);

                    let (read_tx, read_rx) = tokio::sync::mpsc::channel(100);
                    self.udp_read_data_tx_map.lock().insert(src_ipe, read_tx);

                    let sa = Addr {
                        addr: NetAddr::Socket(src_addr),
                        network: Network::UDP,
                    };
                    let ta = Addr {
                        addr: NetAddr::Socket(dst_addr),
                        network: Network::UDP,
                    };

                    let ac = super::udp2::new(sa, sh, read_rx, self.udp_write_data_tx.clone());

                    //debug!("smoltcp got new udp connection {dst_ipe} {src_ipe}");

                    let early_data = BytesMut::from(packet.payload());

                    let _ = self
                        .new_stream_tx
                        .try_send(MapResult::new_u(ac).a(Some(ta)).b(Some(early_data)).build());
                }
            }

            _ => {
                debug!("got unhandled protocol {}", ip_packet.protocol())
            }
        }
    }

    /// check timeout udp and remove from device's SocketSet
    pub fn udp_health_check(&mut self) {
        //debug!("udp_health_check");
        let mut udp_src_to_remove = Vec::new();

        {
            let m = self.udp_read_data_tx_map.lock();

            for (src, sender) in m.iter() {
                if sender.is_closed() {
                    //debug!("udp_health_check got a closed");
                    udp_src_to_remove.push(src.to_owned());
                }
            }
        }

        self.remove_udp_list(udp_src_to_remove);
    }

    /// 名称跟随 smoltcp 的规范.  从smoltcp的 base_conn(tun) 对每个 socket 用 recv_slice 读取数据, 并解析、发送到到实际 TcpStream/UDP的AddrConn 中
    pub fn process_ingress(&mut self) {
        let mut handles_to_remove = Vec::new();
        let mut tcp_src_to_remove = Vec::new();
        let mut udp_src_to_remove = Vec::new();

        //debug!("process_ingress...");

        self.sockets.iter_mut().for_each(|(h, so)| {
            match so {
                smoltcp::socket::Socket::Udp(so) => {
                    /*
                    smoltcp 中, udp 在 client端 的逻辑是反的，它在建立udp socket 时(bind)，只存储目标的ip+port,
                    对 该 socket 进行 recv_slice 时, 得到的地址是 源的ip+port (本地地址)
                     */

                    while so.can_recv() {
                        let mut buffer = BytesMut::with_capacity(MTU);
                        unsafe {
                            buffer.set_len(MTU);
                        }
                        let r1 = so.recv_slice(buffer.as_mut());
                        match r1 {
                            Ok((n, src)) => {
                                unsafe {
                                    buffer.set_len(n);
                                }

                                let m = self.udp_read_data_tx_map.lock();
                                let udp_read_data_sender = match m.get(&src.endpoint) {
                                    Some(s) => s,
                                    None => {
                                        debug!("udp recv from {} but not in map", src.endpoint);
                                        return;
                                    }
                                };

                                let dst = so.endpoint();
                                let dst_ipe: IpEndpoint = IpEndpoint {
                                    addr: dst.addr.unwrap(),
                                    port: dst.port,
                                };

                                let r2 = udp_read_data_sender.try_send((dst_ipe, buffer));
                                if r2.is_err() {
                                    debug!("udp e2 {:?}", r2);
                                    udp_src_to_remove.push(src.endpoint);
                                    break;
                                }
                            }
                            Err(e) => {
                                debug!("udp e1 {e}");

                                handles_to_remove.push(h);
                                break;
                            }
                        }
                    }
                    if !so.is_open() {
                        debug!("udp not open");
                        handles_to_remove.push(h);
                    }
                }
                smoltcp::socket::Socket::Tcp(so) => {
                    if !so.can_recv() {
                        return;
                    }
                    let src = match so.remote_endpoint() {
                        Some(s) => s,
                        None => {
                            handles_to_remove.push(h);
                            return;
                        }
                    };
                    let m = self.tcp_read_data_tx_map.lock();
                    let tcp_stream_read_data_sender = m.get(&src).unwrap();

                    while so.can_recv() && tcp_stream_read_data_sender.capacity() > 0 {
                        let mut buffer = BytesMut::with_capacity(so.recv_queue());
                        unsafe {
                            buffer.set_len(so.recv_queue());
                        }
                        if let Ok(n) = so.recv_slice(buffer.as_mut()) {
                            if n != buffer.len() {
                                tracing::warn!(
                                    "so.recv_slice n != buffer.len(), {n} {}",
                                    buffer.len()
                                );
                                unsafe {
                                    buffer.set_len(n);
                                }
                            }
                            let r = tcp_stream_read_data_sender.try_send(buffer);
                            if let Err(e) = r {
                                tracing::warn!("tcp_read_data_tx send failed, {e}");
                                so.close();
                                break;
                            }
                        } else {
                            tracing::debug!("tcp_read_data_tx so.recv_slice failed");
                            so.close();
                            break;
                        }
                    }
                    if so.state() == smoltcp::socket::tcp::State::CloseWait && so.send_queue() == 0
                    {
                        let _ = tcp_stream_read_data_sender.try_send(BytesMut::with_capacity(0));
                        so.close();
                    }
                    if !so.is_active() {
                        tcp_src_to_remove.push(src);
                    }
                }

                _ => {}
            } //match
        }); //iter

        //debug!("udp count {udp_count}");

        self.remove_tcp_list(tcp_src_to_remove);
        self.remove_udp_list(udp_src_to_remove);

        for handle in handles_to_remove {
            self.sockets.remove(handle);
        }
    }

    /// 名称跟随 smoltcp 的规范. 对socket 的要写的数据 用 send_slice 写入 smoltcp 的 socket 的 buffer,
    /// 之后可调用 iface.poll 来发出.
    pub fn process_tcp_egress(&mut self, sh: SocketHandle, mut data: BytesMut) {
        //debug!("process_egress tcp for {sh}, {}",data.len());

        let socket: &mut smoltcp::socket::tcp::Socket = self.sockets.get_mut(sh);

        if data.is_empty() {
            socket.close();
            return;
        }
        let mut left_data = data.len();

        while left_data > 0 {
            //debug!("while left_data>0, {left_data}");

            let r = socket.send_slice(&data);

            match r {
                Ok(n) => {
                    if n == 0 {
                        //necessary

                        //debug!("n == 0");
                        socket.close();
                        break;
                    } else {
                        left_data -= n;
                        data.advance(n);
                    }
                }
                Err(_) => {
                    socket.close();
                    break;
                }
            }
        }
    }

    /// 名称跟随 smoltcp 的规范. 对socket 的要写的数据 用 send_slice 写入 smoltcp 的 socket 的 buffer,
    /// 之后可调用 iface.poll 来发出.
    pub fn process_udp_egress(&mut self, sh: SocketHandle, src: IpEndpoint, data: BytesMut) {
        //debug!("process_egress udp for {sh}, {src}, {}",data.len());

        let socket: &mut smoltcp::socket::udp::Socket = self.sockets.get_mut(sh);

        if data.is_empty() {
            socket.close();
            return;
        }

        let r = socket.send_slice(&data, src);

        match r {
            Ok(_) => {}
            Err(e) => {
                debug!(
                    "smoltcp send udp failed, dst:{src}, l:{}, e:{e}",
                    data.len()
                );
                socket.close()
            }
        }
    }

    // fn remove_tcp(&mut self, src: IpEndpoint) {
    //     tracing::debug!("remove tcp {}", src);
    //     let mut tcp_src_handle_map_lock = self.tcp_src_handle_map.lock();
    //     if let Some(h) = tcp_src_handle_map_lock.get(&src) {
    //         self.sockets.remove(*h);
    //         tcp_src_handle_map_lock.remove(&src);
    //     }
    //     self.tcp_read_data_tx_map.lock().remove(&src);
    // }

    fn remove_tcp_list(&mut self, list: Vec<IpEndpoint>) {
        //tracing::debug!("remove udp {}", src);

        let mut tcp_src_handle_map_lock = self.udp_src_handle_map.lock();
        let mut tcp_read_data_tx_map_lock = self.tcp_read_data_tx_map.lock();

        for src in list {
            if let Some(h) = tcp_src_handle_map_lock.get(&src) {
                self.sockets.remove(*h);
                tcp_src_handle_map_lock.remove(&src);
            }
            tcp_read_data_tx_map_lock.remove(&src);
        }
    }

    // fn remove_udp(&mut self, src: IpEndpoint) {
    //     tracing::debug!("remove udp {}", src);

    //     let mut udp_src_handle_map_lock = self.udp_src_handle_map.lock();
    //     if let Some(h) = udp_src_handle_map_lock.get(&src) {
    //         self.sockets.remove(*h);
    //         udp_src_handle_map_lock.remove(&src);
    //     }

    //     self.udp_read_data_tx_map.lock().remove(&src);
    // }

    fn remove_udp_list(&mut self, list: Vec<IpEndpoint>) {
        //tracing::debug!("remove udp {}", src);

        let mut udp_src_handle_map_lock = self.udp_src_handle_map.lock();
        let mut udp_read_data_tx_map_lock = self.udp_read_data_tx_map.lock();

        for src in list {
            if let Some(h) = udp_src_handle_map_lock.get(&src) {
                self.sockets.remove(*h);
                udp_src_handle_map_lock.remove(&src);
            }
            udp_read_data_tx_map_lock.remove(&src);
        }
    }
}
