use bytes::BufMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use self::map::{MapParams, ProxyBehavior, CID};
use super::*;
use crate::{map::MapResult, net};

#[derive(Debug, Clone)]
pub struct Client {
    pub up: Option<UserPass>, //todo: make sure len <= 255

    pub use_earlydata: bool, //todo: implement this.
}

impl Client {
    //返回的 extra data 为 server 所选定的 adopted method
    async fn handshake(
        &self,
        cid: CID,
        mut base: net::Conn,
        a: net::Addr,
        b: Option<BytesMut>,
    ) -> io::Result<map::MapResult> {
        //let mut base = params.c;

        const BUFLEN: usize = 1024; //todo: change this.
        let mut buf = BytesMut::with_capacity(BUFLEN);

        let adopted_method = if self.up.is_some() {
            AUTH_PASSWORD
        } else {
            AUTH_NONE
        };
        buf.extend_from_slice(&[VERSION5, 1, adopted_method][..]);

        base.write(&buf[..]).await?;
        buf.resize(BUFLEN, 0);
        let mut n = base.read(&mut buf).await?;

        if n != 2 || buf[0] != VERSION5 || buf[1] != adopted_method {
            return Err(io::Error::other(format!(
                "cid: {}, socks5 client handshake,protocol err, {}",
                cid, buf[1]
            )));
        }

        if adopted_method == AUTH_PASSWORD {
            buf.clear();
            buf.put_u8(1);
            let upr = self.up.as_ref().unwrap();
            buf.put_u8(upr.user.len() as u8);
            buf.put(upr.user.as_bytes());
            buf.put_u8(upr.pass.len() as u8);
            buf.put(upr.pass.as_bytes());

            base.write(&buf).await?;

            buf.resize(BUFLEN, 0);
            n = base.read(&mut buf).await?;

            if n != 2 || buf[0] != 1 || buf[1] != 0 {
                return Err(io::Error::other(format!(
                    "cid: {}, socks5 client handshake,auth failed, {}",
                    cid, buf[1]
                )));
            }
        }
        buf.clear();
        buf.extend_from_slice(&[VERSION5, CMD_CONNECT, 0][..]);
        net::helpers::addr_to_socks5_bytes(&a, &mut buf);
        base.write(&buf).await?;

        buf.resize(BUFLEN, 0);
        n = base.read(&mut buf).await?;

        if n < 10 || buf[0] != 5 || buf[1] != 0 || buf[2] != 0 {
            return Err(io::Error::other(format!(
                "cid: {}, socks5 client handshake failed when reading response",
                cid
            )));
        }

        if let Some(ed) = b {
            base.write_all(&ed).await?;
        }

        Ok(MapResult {
            a: None,
            b: None,
            c: map::Stream::TCP(base),
            d: Some(map::AnyData::B(Box::new(adopted_method))),
            e: None,
        })
    }
}

impl crate::Name for Client {
    fn name(&self) -> &'static str {
        "socks5"
    }
}

#[async_trait::async_trait]
impl map::Mapper for Client {
    async fn maps(
        &self,
        cid: CID, //state 的 id
        _behavior: ProxyBehavior,
        params: MapParams,
    ) -> MapResult {
        if params.a.is_none() {
            return MapResult::err_str(&format!(
                "cid: {}, socks5 outadder called without target_addr",
                cid
            ));
        }

        match params.c {
            map::Stream::TCP(c) => {
                let r = self.handshake(cid, c, params.a.unwrap(), params.b).await;
                MapResult::from_result(r)
            }
            _ => MapResult::err_str("socks5 only support tcplike stream"),
        }
    }
}
