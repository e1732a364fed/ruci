use tokio::sync::Mutex;

use super::*;

#[derive(Default)]
struct UdpMap {
    m: HashMap<net::Addr, Arc<Mutex<UdpSocket>>>,
}

/*
// 这是移植自 verysimple 上的实现，对未发送过数据的新远程地址 使用 新的端口 发送
// 但是这存在问题. 它是基于一个假设：客户端只使用一次 UDP ASSOCIATE。

所以为了使用不同端口，它内部使用了一个映射表，每个远程目标都有一个自己的对应的 udpsocket
和它的读写循环，这种实现增加了复杂性，而且会造成大量端口的占用。

实际的使用情况是，如果有需要，用户会对需要新udp拨号端口的远程目标地址使用一次UDP ASSOCIATE

这样可以保证每个 udp socket 只对同一个远程地址进行 send, 这样用户就能对每个
udp 连接控制关闭

*/

//not a good implementation.
pub async fn loop_listen_udp_for_certain_client(
    cid: u32,
    mut base: Conn,
    client_future_addr: net::Addr,
    user_udpso: UdpSocket,
) -> io::Result<()> {
    debug!("cid: {}, socks5 server, start loop listen udp", cid);
    const CAP: usize = 1500; //todo: change this

    let mut buf = BytesMut::with_capacity(CAP);
    let mut buf2 = BytesMut::with_capacity(CAP);

    let mut user_raddr: IpAddr = client_future_addr.get_ip().unwrap();
    let mut um = UdpMap::default();

    let lock_user_udpso = Arc::new(Mutex::new(user_udpso));

    use futures::FutureExt;
    loop {
        select! {
            /*
            A UDP association terminates when the TCP connection that the UDP
            ASSOCIATE request arrived on terminates.
            */
            result = base.read(&mut buf2).fuse()  =>{
                if result.is_err(){
                    debug!("cid: {}, socks5 server, will end loop listen udp because of the read err of the tcp conn, {}", cid, result.unwrap());

                    break;
                }
            },
            default => {

                buf.resize(CAP, 0);
                let (n, raddr) = lock_user_udpso.lock().await.recv_from(&mut buf).await?;
                if user_raddr.is_unspecified() {
                    user_raddr = raddr.ip();
                } else if !raddr.ip().eq(&user_raddr) {
                    warn!(
                        "cid: {}, socks5 server, got udp data from unknown ip, {}, {}",
                        cid, raddr, n
                    );
                    continue;

                }

                buf.truncate(n);
                let a = decode_udp_diagram(&mut buf)?;
                if let Some(usoref) = um.m.get(&a) {
                    usoref.lock().await.send(&buf).await?;
                } else {
                    //对未发送过数据的新远程地址 使用 新的端口 发送
                    let astr = a.get_addr_str();
                    debug!("cid: {}, socks5 server,got new taget, {}", cid, astr);

                    let uso = UdpSocket::bind("0.0.0.0:0").await?;
                    uso.connect(astr).await?;
                    uso.send(&buf).await?;
                    let arcmuso = Arc::new(Mutex::new(uso));
                    let ac = arcmuso.clone();
                    um.m.insert(a, arcmuso);
                    let user_so = lock_user_udpso.clone();

                    task::spawn(async move {
                        let mut buf_r = BytesMut::with_capacity(CAP);
                        let mut buf_w = BytesMut::with_capacity(CAP);

                        loop{//todo: changethis, add a select, or loop never end
                            let uso = ac.lock().await;
                            buf_r.resize(CAP,0);

                            let (n, raddr) = uso.recv_from(&mut buf_r).await?;
                            buf_r.truncate(n);

                            buf_w.clear();
                            encode_udp_diagram(net::Addr{
                                addr: net::NetAddr::Socket(raddr),
                                network: net::Network::UDP
                            },&mut buf_w,)?;

                            buf_w.extend_from_slice(&buf_r);

                            user_so.lock().await.send(&buf_w).await?;

                            if false{
                                return Ok::<(), Error>(())

                            }
                        }

                    });
                }
            }
        }
    }
    Ok(())
}
