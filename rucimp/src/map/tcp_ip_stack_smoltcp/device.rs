/*!
Defines the [`SmoltcpDevice`] which is a [`Device`] required in [`smoltcp::iface::Interface`].
 */

use bytes::{Buf, BytesMut};
use smoltcp::iface::SocketHandle;
use smoltcp::phy::{Device, RxToken, TxToken};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use ruci::map::*;
use ruci::net::*;

use smoltcp::socket::tcp::{self, State};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::{debug, error, warn};

use anyhow::Context;
use smoltcp::wire::{IpEndpoint, IpProtocol, TcpPacket, UdpPacket};
use std::net::SocketAddr;
use std::task::Poll;

use super::ip_packet::IpPacket;

use dashmap::{DashMap, DashSet, Entry};

const BUF_SIZE: usize = 65535;

//todo: 解决内存泄漏 问题 和 卡顿 问题

/// 通过 SmoltcpDevice 创建一个 smoltcp::iface::Interface, 其在 smoltcp 中是关键
pub(crate) fn create_interface(device: &mut SmoltcpDevice) -> smoltcp::iface::Interface {
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

/// to record a received message，created by Device's receive method.
pub(crate) struct MyRxToken<'a> {
    data: &'a mut [u8],
}
impl RxToken for MyRxToken<'_> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(self.data)
    }
}

/// send msg，created by Device's transmit (and receive) method.
pub(crate) struct MyTxToken {
    tx: Sender<BytesMut>,
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

#[derive(Default)]
pub enum NewReadType {
    TCP(SocketHandle),
    UDP(SocketHandle),
    #[default]
    None,
}

/// 实现 smoltcp 的 Device trait.
///
/// Device trait 的 receive 方法会在 smoltcp 的 iface.poll 被调用 后自动触发.
///
/// 主要 存放的是 sockets, 以及对应的几个 channel 以与 每个 TcpStream 收发信息
pub(crate) struct SmoltcpDevice {
    pub data: DeviceData,

    r: tokio::io::ReadHalf<Conn>,

    /// base_conn read state
    r_state: Poll<usize>,
    rbuf: Box<[u8; u16::MAX as usize]>,

    device_write_tx: Sender<BytesMut>,

    /// 生成新 Stream 后由此发出.
    new_stream_tx: tokio::sync::mpsc::Sender<MapResult>,

    /// only here to be cloned for new TcpStream
    tcp_write_data_tx: Sender<(SocketHandle, SocketAddr, BytesMut)>,

    /// only here to be cloned for new UdpStream
    udp_write_data_tx: Sender<(SocketHandle, IpEndpoint, BytesMut)>,

    /// SocketSet 是 smoltcp 提供的socket 的容器, 用 socket 对应的
    /// handle 来提取 某个 socket
    ///
    /// 在程序运行中，在 iface.poll 之后，我们总是要遍历 所有的 socket，
    /// 并查看每个 socket 的状态, 在可读时用 recv_slice （process_ingress 中），
    /// 在可写时用 send_slice （process_egress 中)
    ///
    /// 这个 SocketSet 比较坑，没有check 方法: 一旦get/remove 时 不存在, 就会 panic
    pub tcp_sockets: smoltcp::iface::SocketSet<'static>,
    pub udp_sockets: smoltcp::iface::SocketSet<'static>,
    // 用于流量记录
    //traffic: Traffic,
}

pub struct DeviceData {
    cid: CID,
    pub new_read_handle: NewReadType,

    tcp_handle_set: DashSet<SocketHandle>,
    udp_handle_set: DashSet<SocketHandle>,

    tcp_src_handle_map: DashMap<IpEndpoint, SocketHandle>,
    tcp_handle_src_map: DashMap<SocketHandle, IpEndpoint>,

    udp_src_handle_map: DashMap<IpEndpoint, SocketHandle>,
    udp_handle_src_map: DashMap<SocketHandle, IpEndpoint>,

    tcp_read_data_tx_map: DashMap<SocketHandle, Sender<BytesMut>>,
    udp_read_data_tx_map: DashMap<IpEndpoint, Sender<(IpEndpoint, BytesMut)>>,
}

impl Default for DeviceData {
    fn default() -> Self {
        Self {
            cid: CID::default(),
            new_read_handle: NewReadType::default(),
            tcp_handle_set: DashSet::new(),
            udp_handle_set: DashSet::new(),
            tcp_src_handle_map: DashMap::new(),
            tcp_handle_src_map: DashMap::new(),
            udp_src_handle_map: DashMap::new(),
            udp_handle_src_map: DashMap::new(),
            tcp_read_data_tx_map: DashMap::new(),
            udp_read_data_tx_map: DashMap::new(),
        }
    }
}

impl Device for SmoltcpDevice {
    type RxToken<'a>
        = MyRxToken<'a>
    where
        Self: 'a;

    type TxToken<'a>
        = MyTxToken
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
                let data = &mut self.rbuf[..n];

                let rx = MyRxToken { data };
                let tx = MyTxToken {
                    tx: self.device_write_tx.clone(),
                };
                self.r_state = Poll::Pending;
                Some((rx, tx))
            }
        }
    }

    fn transmit(&mut self, _timestamp: smoltcp::time::Instant) -> Option<Self::TxToken<'_>> {
        Some(MyTxToken {
            tx: self.device_write_tx.clone(),
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
pub(crate) struct DeviceAndReceivers {
    pub iface: smoltcp::iface::Interface,
    pub device: SmoltcpDevice,
    pub tcp_rx: Receiver<(SocketHandle, SocketAddr, BytesMut)>,
    pub udp_rx: Receiver<(SocketHandle, IpEndpoint, BytesMut)>,
}

/// 返回 SmoltcpDevice，和 接收 tcp 写新信息 的 Receiver 和 接收 udp 写新信息 的 Receiver
///
///  接受的 net_stream_tx 将被用于向外发送 从 base_conn 新解析出的 tcp/udp stream.
pub(crate) fn create(
    cid: CID,
    base_conn: ruci::net::Conn,
    new_stream_tx: tokio::sync::mpsc::Sender<MapResult>,
) -> DeviceAndReceivers {
    let (device_write_tx, mut device_write_rx) = mpsc::channel(100);

    let (tcp_write_data_tx, tcp_write_data_rx) = mpsc::channel(100);
    let (udp_write_data_tx, udp_write_data_rx) = mpsc::channel(100);

    let (r, mut w) = tokio::io::split(base_conn);

    let mut device = SmoltcpDevice {
        r,
        device_write_tx,

        rbuf: Box::new([0; u16::MAX as usize]),
        r_state: Poll::Pending,

        new_stream_tx,

        tcp_sockets: smoltcp::iface::SocketSet::new([]),
        udp_sockets: smoltcp::iface::SocketSet::new([]),

        data: Default::default(),

        tcp_write_data_tx,
        udp_write_data_tx,
    };
    device.data.cid = cid;

    tokio::spawn(async move {
        loop {
            let ob = device_write_rx.recv().await;
            match ob {
                Some(b) => {
                    if let Err(e) = w.write_all(&b).await {
                        debug!("smoltcp write got e {e}, will break.");
                        break;
                    }
                }
                None => {
                    debug!("smoltcp write got None, will break.");
                    break;
                }
            }
        }
    });
    let iface = create_interface(&mut device);

    DeviceAndReceivers {
        iface,
        device,
        tcp_rx: tcp_write_data_rx,
        udp_rx: udp_write_data_rx,
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
        self.check_read_buf_for_new_conn(n);

        self.r_state = Poll::Ready(n);

        Ok(())
    }

    /// 被 Device trait 的 receive 方法调用, 检查 self.buf,
    /// 判断是否有新 tcp 产生，如有, 建立新 TcpStream 并 送入 new_stream_tx, 并创建新的 sockethandle 放入 sockets，
    /// 如果是旧的tcp ，将该tcp 的 sockethandle 记录，以便后面调用的 process_ingress 使用它。
    /// udp 同理
    fn check_read_buf_for_new_conn(&mut self, n: usize) {
        //debug!("check_read_buf_for_new_conn {n}");

        let packet =
            IpPacket::new_checked(&self.rbuf[..n]).context("convert frame to IpPacket failed");
        let ip_packet = match packet {
            Ok(p) => p,
            Err(e) => {
                warn!(cid = %self.data.cid, "smoltcp check_read_buf_for_new_conn got err: {e}");
                return;
            }
        };
        let src_ip_addr = ip_packet.src_addr();
        let dst_ip_addr = ip_packet.dst_addr();

        match ip_packet.protocol() {
            IpProtocol::Icmp | IpProtocol::Icmpv6 => {
                //debug!("is icmp, {n} {}",ip_packet.payload().len())
                self.data.new_read_handle = NewReadType::None;
            }
            IpProtocol::Tcp => {
                //debug!("is tcp, {n} {}",ip_packet.payload().len());
                let tcp_packet = match TcpPacket::new_checked(ip_packet.payload()) {
                    Ok(p) => p,
                    Err(err) => {
                        warn!(cid = %self.data.cid,
                            "smoltcp check_read_buf_for_new_conn got invalid TCP packet err: {}, src_ip: {}, dst_ip: {}, payload: {}",
                            err,
                            src_ip_addr,
                            dst_ip_addr,
                            ruci::utils::HexSlice(ip_packet.payload())
                        );
                        self.data.new_read_handle = NewReadType::None;
                        return;
                    }
                };

                // 判断一个TcpPacket 是否为一个新的Tcp Stream 的第一条信息
                // #[inline]
                // fn is_tcp_client_hello(tcp_packet: &TcpPacket<&[u8]>) -> bool {
                //     tcp_packet.syn() && !tcp_packet.ack()
                // }

                // let not_tcp_client_hello = !is_tcp_client_hello(&tcp_packet);
                // if !is_tcp_client_hello(&tcp_packet) {
                //     return;
                // }
                let src_port = tcp_packet.src_port();
                let dst_port = tcp_packet.dst_port();

                let src_addr = SocketAddr::new(src_ip_addr, src_port);
                let dst_addr = SocketAddr::new(dst_ip_addr, dst_port);

                let ipe = src_addr.into();

                match self.data.tcp_src_handle_map.entry(ipe) {
                    Entry::Occupied(occupied_entry) => {
                        self.data.new_read_handle = NewReadType::TCP(*occupied_entry.get());
                    }
                    Entry::Vacant(e) => {
                        let mut new_tcp_socket = tcp::Socket::new(
                            tcp::SocketBuffer::new(vec![0; BUF_SIZE]),
                            tcp::SocketBuffer::new(vec![0; BUF_SIZE]),
                        );
                        new_tcp_socket.set_timeout(Some(smoltcp::time::Duration::from_secs(7200)));
                        if let Err(err) = new_tcp_socket.listen(dst_addr) {
                            warn!("tcp listen error: {:?}", err);
                            return;
                        }

                        new_tcp_socket.set_nagle_enabled(false);
                        new_tcp_socket.set_ack_delay(None);

                        let socket_handle = self.tcp_sockets.add(new_tcp_socket);
                        self.data.tcp_handle_set.insert(socket_handle);
                        e.insert(socket_handle);

                        self.data.tcp_handle_src_map.insert(socket_handle, ipe);

                        let (read_tx, read_rx) = tokio::sync::mpsc::channel(100);
                        self.data
                            .tcp_read_data_tx_map
                            .insert(socket_handle, read_tx);
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

                match self.data.udp_src_handle_map.entry(src_ipe) {
                    Entry::Occupied(occupied_entry) => {
                        self.data.new_read_handle = NewReadType::UDP(*occupied_entry.get());
                    }
                    Entry::Vacant(e) => {
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
                        let sh = self.udp_sockets.add(socket);
                        self.data.udp_handle_set.insert(sh);
                        e.insert(sh);
                        self.data.new_read_handle = NewReadType::UDP(sh);
                        self.data.udp_handle_src_map.insert(sh, src_ipe);

                        let (read_tx, read_rx) = tokio::sync::mpsc::channel(100);
                        self.data.udp_read_data_tx_map.insert(src_ipe, read_tx);

                        let sa = Addr {
                            addr: NetAddr::Socket(src_addr),
                            network: Network::UDP,
                        };
                        let ta = Addr {
                            addr: NetAddr::Socket(dst_addr),
                            network: Network::UDP,
                        };

                        let ac = super::udp::new(sa, sh, read_rx, self.udp_write_data_tx.clone());

                        //debug!("smoltcp got new udp connection {dst_ipe} {src_ipe}");

                        let early_data = BytesMut::from(packet.payload());

                        let _ = self
                            .new_stream_tx
                            .try_send(MapResult::new_u(ac).a(Some(ta)).b(Some(early_data)).build());
                    }
                }
            }

            _ => {
                debug!("got unhandled protocol {}", ip_packet.protocol());
                self.data.new_read_handle = NewReadType::None;
            }
        }
    }

    /// check timeout udp and remove from device's SocketSet
    pub fn udp_health_check(&mut self) {
        // debug!("udp_health_check");
        let mut udp_src_to_remove = Vec::new();

        {
            let m = &self.data.udp_read_data_tx_map;

            for ref_multi in m.iter() {
                let sender = ref_multi.value();
                // debug!("checking udp for {:?}", src);
                if sender.is_closed() {
                    //debug!("udp_health_check got a closed");
                    udp_src_to_remove.push(ref_multi.key().to_owned());
                }
            }
        }

        self.remove_udp_list(&vec![], udp_src_to_remove);
    }

    /// 名称跟随 smoltcp 的规范.  对当前 socket 用 recv_slice 读取数据, 并解析、发送到到实际 TcpStream/UDP的AddrConn 中
    pub fn process_ingress(&mut self) {
        //debug!("process_ingress...");

        match self.data.new_read_handle {
            NewReadType::TCP(h) => {
                let mut tcp_handles_to_remove = Vec::new();
                let mut tcp_src_to_remove = Vec::new();

                if !self.data.tcp_handle_set.contains(&h) {
                    return;
                }

                let so: &mut smoltcp::socket::tcp::Socket = self.tcp_sockets.get_mut(h);

                // debug!(
                //     "checking {h}, {:?}, {:?} {}",
                //     so.local_endpoint(),
                //     so.remote_endpoint(),
                //     so.state()
                // );
                if !so.can_recv() {
                    debug!("cant recv {h}");
                }

                if so.state() == State::Closed {
                    debug!("{h}, closed, push to remove");
                    tcp_handles_to_remove.push(h);
                }

                let src = match so.remote_endpoint() {
                    Some(s) => s,
                    None => {
                        return;
                    }
                };

                if let Some(tcp_stream_read_data_sender) = self.data.tcp_read_data_tx_map.get(&h) {
                    if tcp_stream_read_data_sender.is_closed() {
                        tcp_handles_to_remove.push(h);
                        if let Some(src) = self.data.tcp_handle_src_map.get(&h) {
                            tcp_src_to_remove.push(*src.value());
                        }
                    } else {
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
                                // debug!("tcp_read_data_tx so.recv_slice failed");
                                so.close();
                                break;
                            }
                        }
                        if so.state() == State::CloseWait && so.send_queue() == 0 {
                            let _ =
                                tcp_stream_read_data_sender.try_send(BytesMut::with_capacity(0));
                            //这里将令 TcpStream 的 read端 收到一个 0字节，表示EOF
                            so.close();
                        }
                        if !so.is_active() {
                            debug!("{h}, !so.is_active()");
                            tcp_handles_to_remove.push(h);
                            tcp_src_to_remove.push(src);
                        }
                    }
                }

                self.remove_tcp_list(&tcp_handles_to_remove, tcp_src_to_remove);
            }
            NewReadType::UDP(h) => {
                let mut udp_handles_to_remove = Vec::new();
                let mut udp_src_to_remove = Vec::new();

                let so: &mut smoltcp::socket::udp::Socket = self.udp_sockets.get_mut(h);

                /*
                smoltcp 中, udp 在 client端 的逻辑是反的，它在建立udp socket 时(bind)，只存储目标的ip+port,
                对 该 socket 进行 recv_slice 时, 得到的地址是 源的ip+port (本地地址)
                 */

                while so.can_recv() {
                    let mut buffer = BytesMut::with_capacity(MTU);
                    unsafe {
                        buffer.set_len(MTU);
                    }
                    match so.recv_slice(buffer.as_mut()) {
                        Err(e) => {
                            debug!("udp e1 {e}");

                            udp_handles_to_remove.push(h);
                            break;
                        }
                        Ok((n, src)) => {
                            unsafe {
                                buffer.set_len(n);
                            }

                            let m = &self.data.udp_read_data_tx_map;
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
                                udp_handles_to_remove.push(h);
                                break;
                            }
                        }
                    }
                }
                if !so.is_open() {
                    //这里 smoltcp 中的代码只是检测 endpoint.port 不为0，因此实际在本代码实现中不会被触发
                    debug!("udp not open");
                    udp_handles_to_remove.push(h);
                }

                self.remove_udp_list(&udp_handles_to_remove, udp_src_to_remove);
            }
            //debug!("udp count {udp_count}");
            NewReadType::None => {}
        }
    }

    /// 发送tcp 数据
    ///
    ///  对socket 的要写的数据 用 send_slice 写入 smoltcp 的 socket 的 buffer,
    /// 之后可调用 iface.poll 来发出.
    pub fn send_tcp(&mut self, sh: SocketHandle, mut data: BytesMut) {
        // debug!("process_egress tcp for {sh}, {}", data.len());

        if !self.data.tcp_handle_set.contains(&sh) {
            // 有可能是在 close、把 handle 删掉后，又调用了一次 tcpstream的 shutdown，此时就是没有 handle 的情况,
            // 应直接返回
            return;
        }

        let socket: &mut smoltcp::socket::tcp::Socket = self.tcp_sockets.get_mut(sh);

        if data.is_empty() {
            socket.close();
            return;
        }
        let mut left_data = data.len();

        // let mut count = 0;
        while left_data > 0 {
            // debug!("while left_data>0, {left_data}, {count}"); //一般只会发送一次
            // count += 1;

            match socket.send_slice(&data) {
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
                Err(e) => {
                    tracing::error!("smoltcp Failed to send TCP data: {e}");
                    socket.close();
                    break;
                }
            }
        }
    }

    /// 发送udp 数据
    ///
    ///  对socket 的要写的数据 用 send_slice 写入 smoltcp 的 socket 的 buffer,
    /// 之后可调用 iface.poll 来发出.
    pub fn send_udp(&mut self, sh: SocketHandle, src: IpEndpoint, data: BytesMut) {
        //debug!("process_egress udp for {sh}, {src}, {}",data.len());

        let socket: &mut smoltcp::socket::udp::Socket = self.udp_sockets.get_mut(sh);

        if data.is_empty() {
            socket.close();
            return;
        }

        if let Err(e) = socket.send_slice(&data, src) {
            error!(
                "smoltcp send udp failed, dst:{src}, l:{}, e:{e}",
                data.len()
            );
            socket.close()
        }
    }

    fn remove_tcp_list(&mut self, sh_list: &Vec<SocketHandle>, mut src_list: Vec<IpEndpoint>) {
        // let is_removing = !sh_list.is_empty() || !src_list.is_empty();

        if !sh_list.is_empty() && src_list.is_empty() {
            for h in sh_list {
                if let Some(src) = self.data.tcp_handle_src_map.get(h) {
                    src_list.push(*src.value());
                }
            }
        }

        if !src_list.is_empty() {
            for src in src_list {
                self.data.tcp_src_handle_map.remove(&src);
            }
        }

        for h in sh_list {
            self.data.tcp_read_data_tx_map.remove(h);
            self.data.tcp_handle_src_map.remove(h);
            self.tcp_sockets.remove(*h);
            self.data.tcp_handle_set.remove(h);
        }
    }

    fn remove_udp_list(&mut self, sh_list: &Vec<SocketHandle>, mut src_list: Vec<IpEndpoint>) {
        if !sh_list.is_empty() && src_list.is_empty() {
            for h in sh_list {
                if let Some(src) = self.data.udp_handle_src_map.get(h) {
                    src_list.push(*src.value());
                }
            }
        }

        if !src_list.is_empty() {
            for src in src_list {
                self.data.udp_src_handle_map.remove(&src);
                self.data.udp_read_data_tx_map.remove(&src);
            }
        }

        for handle in sh_list {
            self.udp_sockets.remove(*handle);
            self.data.udp_handle_set.remove(handle);
            self.data.udp_handle_src_map.remove(handle);
        }
    }
}
