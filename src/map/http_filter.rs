/*!
http_filter 过滤http1.1信息，并原样返回

一般用于 前置于 websocket 层 或 grpc 层，提供预过滤以用于 回落

http_filter 与 http_proxy 完全不同，不要搞混

 */

use crate::{map, net};
use anyhow::anyhow;
use async_trait::async_trait;
use bytes::BytesMut;
use macro_mapper::NoMapperExt;
use net::http::parse_h1_request;
use tokio::io::AsyncReadExt;

use super::{http::CommonConfig, MapResult, Mapper, ProxyBehavior};

#[derive(Clone, Debug, NoMapperExt, Default)]
pub struct Server {
    pub config: Option<CommonConfig>,
}
impl crate::Name for Server {
    fn name(&self) -> &str {
        "http_filter"
    }
}
#[async_trait]
impl Mapper for Server {
    async fn maps(
        &self,
        cid: net::CID,
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        let conn = params.c;
        if let net::Stream::Conn(mut conn) = conn {
            let mut buf = BytesMut::zeroed(net::http::MAX_PARSE_URL_LEN);

            let r = conn.read(&mut buf).await;
            let u = match r {
                Ok(u) => u,
                Err(e) => return MapResult::from_e(e),
            };
            buf.truncate(u);

            let result = parse_h1_request(&buf, false);
            if result.parse_result != Ok(()) {
                return MapResult::from_e(anyhow!(
                    "http_filter parse failed {:?}",
                    result.parse_result
                ));
            }

            if let Some(c) = &self.config {
                if !c.authority.is_empty() {
                    let given_host = result.get_first_header_by("Host");

                    if c.authority != given_host {
                        let e = anyhow!(
                            "http_filter got wrong host, cid={}, given={}, expected={}",
                            cid,
                            given_host,
                            c.authority
                        );
                        return MapResult::ebc(e, buf, conn);
                    }
                }
                if c.path != result.path {
                    let e = anyhow!(
                        "http_filter got wrong path, cid={}, given={}, expected={}",
                        cid,
                        result.path,
                        c.path
                    );
                    return MapResult::ebc(e, buf, conn);
                }
            }

            MapResult::new_c(conn).b(Some(buf)).a(params.a).build()
        } else {
            MapResult::err_str("http_filter only support tcplike stream")
        }
    }
}
