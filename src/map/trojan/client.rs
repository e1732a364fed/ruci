use anyhow::bail;
use async_trait::async_trait;
use bytes::{BufMut, BytesMut};
use macro_mapper::{mapper_ext_fields, MapperExt};
use tokio::io::AsyncWriteExt;
use tracing::debug;

use crate::{
    map::{self, MapResult, Mapper, MapperExt, CID},
    net::{self, helpers, Network},
    Name,
};

use super::*;

#[mapper_ext_fields]
#[derive(Debug, Clone, MapperExt, Default)]
pub struct Client {
    pub u: User,
}

impl Client {
    pub fn new(plain_text_password: &str) -> Self {
        let u = User::new(plain_text_password);
        Client {
            u,
            ..Default::default()
        }
    }

    pub async fn handshake(
        &self,
        _cid: CID,
        mut base: net::Conn,
        ta: net::Addr,
        mut first_payload: Option<BytesMut>,
    ) -> anyhow::Result<MapResult> {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put(self.u.hex.as_bytes());
        buf.put_u16(CRLF);

        let mut is_udp = false;
        match ta.network {
            Network::TCP => buf.put_u8(CMD_CONNECT),
            Network::UDP => {
                buf.put_u8(CMD_UDPASSOCIATE);
                is_udp = true;
            }
            _ => bail!(
                "trojan client handshake doesn't support this target network: {}",
                ta.network
            ),
        };

        helpers::addr_to_socks5_bytes(&ta, &mut buf);
        buf.put_u16(CRLF);

        if self.is_tail_of_chain() && !is_udp {
            if let Some(b) = &first_payload {
                if !b.is_empty() {
                    let bl = b.len();
                    buf.extend_from_slice(b);
                    first_payload = None;
                    debug!("trojan client writing ed {}", bl);
                }
            }
        }
        base.write_all(&buf).await?;
        base.flush().await?;

        if is_udp {
            let u = udp::from(base);
            Ok(MapResult::new_u(u).b(first_payload).build())
        } else {
            Ok(MapResult::new_c(base).b(first_payload).build())
        }
    }
}
impl Name for Client {
    fn name(&self) -> &'static str {
        "trojan_client"
    }
}

#[async_trait]
impl Mapper for Client {
    async fn maps(
        &self,
        cid: CID,
        _behavior: map::ProxyBehavior,
        params: map::MapParams,
    ) -> MapResult {
        match params.c {
            map::Stream::Conn(c) => {
                if let Some(a) = params.a {
                    let r = self.handshake(cid, c, a, params.b).await;
                    MapResult::from_result(r)
                } else {
                    MapResult::err_str("trojan client requires a target_addr, got None")
                }
            }
            _ => MapResult::err_str("trojan only support tcplike stream"),
        }
    }
}
