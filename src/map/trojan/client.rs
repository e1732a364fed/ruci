use async_trait::async_trait;
use bytes::{BufMut, BytesMut};
use macro_mapper::{common_mapper_field, CommonMapperExt};
use tokio::io::AsyncWriteExt;

use crate::{
    map::{self, MapResult, Mapper, CID},
    net::{self, helpers, Network},
    Name,
};

use super::*;

#[common_mapper_field]
#[derive(Debug, Clone, CommonMapperExt, Default)]
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
        first_payload: Option<BytesMut>,
    ) -> anyhow::Result<MapResult> {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put(self.u.hex.as_bytes());
        buf.put_u16(CRLF);

        if ta.network == Network::TCP {
            buf.put_u8(CMD_CONNECT);
        } else {
            todo!()
        }
        helpers::addr_to_socks5_bytes(&ta, &mut buf);
        buf.put_u16(CRLF);
        if self.is_tail_of_chain {
            if let Some(b) = first_payload {
                buf.extend_from_slice(&b);
            }
            base.write_all(&buf).await?;
            base.flush().await?;

            Ok(MapResult::c(base))
        } else {
            base.write_all(&buf).await?;
            base.flush().await?;

            Ok(MapResult::obc(first_payload, base))
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
            map::Stream::TCP(c) => {
                let r = self
                    .handshake(cid, c, params.a.expect("params has target addr"), params.b)
                    .await;
                MapResult::from_result(r)
            }
            _ => MapResult::err_str("trojan only support tcplike stream"),
        }
    }
}
