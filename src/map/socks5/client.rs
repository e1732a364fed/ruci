use bytes::BufMut;
use macro_mapper::NoMapperExt;
use map::{helpers, Addr, Network};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UdpSocket,
};

use self::map::{MapParams, ProxyBehavior, CID};
use super::*;
use crate::{
    map::{socks5::udp::new_addr_conn, MapResult, MapperExt},
    net,
};
use anyhow::{anyhow, bail, Ok};

#[derive(Debug, Clone, NoMapperExt)]
pub struct Client {
    pub up: Option<PlainText>, //todo: make sure len <= 255

    pub use_earlydata: bool, //todo: implement this.
}

impl Client {
    ///返回的 extra data 为 server 所选定的 adopted method
    async fn handshake(
        &self,
        cid: CID,
        mut base: net::Conn,
        a: net::Addr,
        b: Option<BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        //let mut base = params.c;
        let isudp = a.network == Network::UDP;
        if !a.network.is_tcp_or_udp() {
            bail!(
                "trojan client: target_addr's network can't be proxied: {} ",
                a.network
            )
        }

        const BUFLEN: usize = 1024; //todo: change this.
        let mut buf = BytesMut::with_capacity(BUFLEN);

        let adopted_method = if self.up.is_some() {
            AUTH_PASSWORD
        } else {
            AUTH_NONE
        };
        buf.extend_from_slice(&[VERSION5, 1, adopted_method][..]);

        base.write_all(&buf[..]).await?;
        buf.resize(BUFLEN, 0);
        let mut n = base.read(&mut buf).await?;

        if n != 2 || buf[0] != VERSION5 || buf[1] != adopted_method {
            return Err(anyhow!(
                "{}, socks5 client handshake,protocol err, {}",
                cid,
                buf[1]
            ));
        }

        if adopted_method == AUTH_PASSWORD {
            buf.clear();
            buf.put_u8(1);
            let upr = self
                .up
                .as_ref()
                .expect("self up is some when sever returns adopted_method == AUTH_PASSWORD");
            buf.put_u8(upr.user.len() as u8);
            buf.put(upr.user.as_bytes());
            buf.put_u8(upr.pass.len() as u8);
            buf.put(upr.pass.as_bytes());

            base.write_all(&buf).await?;

            buf.resize(BUFLEN, 0);
            n = base.read(&mut buf).await?;

            if n != 2 || buf[0] != 1 || buf[1] != 0 {
                return Err(anyhow!(
                    "{}, socks5 client handshake,auth failed, {}",
                    cid,
                    buf[1]
                ));
            }
        }
        buf.clear();

        let mut the_ed = b;

        match self.get_pre_defined_early_data() {
            Some(bf) => match the_ed {
                Some(mut ed) => {
                    ed.extend_from_slice(&bf);
                    the_ed = Some(ed);
                }
                None => the_ed = Some(bf),
            },
            None => {}
        }

        if isudp {
            buf.extend_from_slice(&[VERSION5, CMD_UDPASSOCIATE, 0][..]);
            net::helpers::addr_to_socks5_bytes(&Addr::default(), &mut buf);
            base.write_all(&buf).await?;

            buf.resize(buf.capacity(), 0);

            let n = base.read(&mut buf).await?;

            if n < 3 {
                bail!("socks5 client udp handshake read failed, too short: {}", n)
            }
            assert!(n < 3);
            if buf[0] != VERSION5 || buf[1] != 0 || buf[2] != 0 {
                bail!("socks5 client udp handshake read failed, wrong msg");
            }
            buf.truncate(n);
            let server_udp_addr = match helpers::socks5_bytes_to_addr(&mut buf) {
                std::result::Result::Ok(a) => a,
                Err(e) => return Err(e.context("socks5 client udp handshake failed")),
            };

            let server_udp_so = match server_udp_addr.get_socket_addr() {
                Some(so) => so,
                None => bail!("socks5 client udp handshake failed, got server addr, but not a valid socketaddr, is {} instead", &server_udp_addr),
            };
            let uso = UdpSocket::bind(server_udp_so).await?;

            let ac = new_addr_conn(uso, server_udp_so);

            Ok(MapResult::newu(ac)
                .b(the_ed)
                .d(map::AnyData::B(Box::new(adopted_method)))
                .build())
        } else {
            buf.extend_from_slice(&[VERSION5, CMD_CONNECT, 0][..]);
            net::helpers::addr_to_socks5_bytes(&a, &mut buf);
            base.write_all(&buf).await?;

            buf.resize(BUFLEN, 0);
            n = base.read(&mut buf).await?;

            if n < 10 || buf[0] != 5 || buf[1] != 0 || buf[2] != 0 {
                return Err(anyhow!(
                    "{}, socks5 client handshake failed when reading response",
                    cid
                ));
            }

            if let Some(ed) = &the_ed {
                if self.is_tail_of_chain() {
                    base.write_all(&ed).await?;
                    the_ed = None
                }
            }

            Ok(MapResult::newc(base)
                .b(the_ed)
                .d(map::AnyData::B(Box::new(adopted_method)))
                .build())
        }
    }
}

impl crate::Name for Client {
    fn name(&self) -> &'static str {
        "socks5_client"
    }
}

#[async_trait::async_trait]
impl map::Mapper for Client {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let target_addr = match params.a {
            Some(ta) => ta,
            None => {
                return MapResult::err_str(&format!(
                    "{}, socks5 client called without target_addr",
                    cid
                ));
            }
        };

        match params.c {
            map::Stream::TCP(c) => {
                let r = self.handshake(cid, c, target_addr, params.b).await;
                MapResult::from_result(r)
            }
            _ => MapResult::err_str(&format!(
                "socks5 client only support tcplike stream, got {}",
                params.c
            )),
        }
    }
}
