/*!
 * Define a MITM map, which unwrap tls stream at server.
 *
 * 原理是，解析用户的数据，若为 tls client hello, 则 对每一个 client_hello 中的 host 都生成一个 证书，
 * 该证书 是一个 用预定义的 根证书 CA 签名的
 * 然后用这个证书和私钥对用户的请求进行 tls 握手。
 *
 * 这样就可以在中间解析用户的请求，然后再转发给真正的服务器。
 *
 * 前提是，用户的 tls 请求 程序（如浏览器、系统） 是 信任我们的 CA 证书的。
 *
 * 如果 用户数据不是 tls client hello, 则原样传递下去。
 */

use ::http::uri::Authority;
use anyhow::anyhow;
use async_trait::async_trait;
use bytes::BytesMut;
use macro_map::{map_ext_fields, MapExt};
use ruci::map::*;
use ruci::net::helpers::EarlyDataWrapper;
use ruci::net::CID;
use ruci::Name;
use ruci::{map, net::MTU};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, info};

#[map_ext_fields]
#[derive(Debug, Clone, MapExt)]
pub struct MITM {
    pub sc: crate::server::ServerPEMOptions,
}

impl Name for MITM {
    fn name(&self) -> &'static str {
        "mitm_server"
    }
}

#[async_trait]
impl Map for MITM {
    async fn maps(&self, _cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match behavior {
            //server 端
            ProxyBehavior::DECODE | ProxyBehavior::UNSPECIFIED => {
                //把 用户的 params.c 中的 tls 连接 用 rustls 解包，返回解包后的 链接

                let mut c = match params.c.try_unwrap_tcp() {
                    Ok(c) => c,
                    Err(e) => return MapResult::from_e(e),
                };

                let b = if params.b.is_some() && params.b.as_ref().unwrap().len() >= 4 {
                    params.b.unwrap()
                } else {
                    let mut b = [0u8; MTU];

                    let r = tokio::time::timeout(std::time::Duration::from_secs(5), c.read(&mut b))
                        .await;

                    match r {
                        Ok(r) => match r {
                            Ok(n) => {
                                if n == 0 {
                                    return MapResult::from_e(anyhow!(
                                        "MITM: no data read from client, ta: {:?}",
                                        params.a
                                    ));
                                }
                                if n < 2 {
                                    return MapResult::from_err_str(
                                        "MITM: only read less than 4 bytes, too short",
                                    );
                                }
                                BytesMut::from(&b[..n])
                            }
                            Err(e) => {
                                return MapResult::from_e(
                                    anyhow::Error::from(e)
                                        .context(format!("MITM: ta {:?} read failed", params.a)),
                                )
                            }
                        },
                        Err(_) => {
                            info!("MITM: read client first data timeout, passing as is");
                            return MapResult::new_c(c).a(params.a).build();
                        }
                    }
                };

                debug_assert!(b.len() >= 4);

                if b[..2] == *b"\x16\x03" {
                    let authority = if let Some(host) = crate::extract_host_from_client_hello(&b) {
                        host
                    } else {
                        panic!("MITM: No SNI host found");
                    };

                    // let alpn = { tls::extract_alpn_from_clinet_hello(&b) };
                    // debug!("MITM: client shown alpn is {:?}", alpn);

                    let sc = crate::load::load_ser_config_from_pem(
                        &self.sc,
                        Some(&Authority::from_maybe_shared(authority.clone()).unwrap()),
                    )
                    .unwrap();

                    let ec = EarlyDataWrapper::from(b, c);

                    let mut stream = match tokio_rustls::TlsAcceptor::from(Arc::new(sc))
                        .accept(ec)
                        .await
                    {
                        Ok(stream) => Box::new(stream),
                        Err(e) => {
                            let e = anyhow!("MITM: Failed to establish TLS connection: {}", e);
                            return MapResult::from_e(e);
                        }
                    };

                    // params.a and authority should be the same

                    let ta = if let Some(a) = params.a {
                        if a.get_name().unwrap() != authority {
                            let e = anyhow!(
                                "MITM: authority not match, ta: {:?}, authority: {}",
                                a,
                                authority
                            );
                            error!("{}", e);

                            std::process::exit(1);
                        } else {
                            debug!(
                                "MITM TLS connection established, ta: {:?}, authority: {}",
                                a, authority
                            );
                        }

                        Some(a)
                    } else {
                        debug!(
                            "MITM TLS connection established, no ta given, authority: {}",
                            authority
                        );

                        Some(ruci::net::Addr::from_addr_str("tcp", &(authority + ":443")).unwrap())
                    };

                    // 重新读取一次，得到 tls 的首包用户数据

                    let mut b = [0u8; MTU * 2];
                    let n = match stream.read(&mut b).await {
                        Ok(n) => {
                            debug!(
                                "MITM: first read client data success, len: {}, content: {}",
                                n,
                                unsafe { String::from_utf8_unchecked(b[..n.min(100)].to_vec()) }
                            );

                            // 常见的 n 的值在 1797 左右. 500 到两千多 都是有可能的。

                            // 其主要是 GET 请求，最占空间的地方是 url 和 cookie

                            //h2: preface 24字节: PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n

                            // let has = b[..n]
                            //     .windows(24)
                            //     .position(|window| window == b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");

                            // if has.is_some() {
                            //     debug!("  connection is h2");

                            //     crate::map::h2::parse_frames(&b[24..n]).unwrap();
                            // }

                            n
                        }
                        Err(e) => {
                            return MapResult::from_e(anyhow!(
                                "MITM: first read client data failed: {}",
                                e
                            ))
                        }
                    };

                    let b = BytesMut::from(&b[..n]);

                    return MapResult::new_c(stream).a(ta).b(Some(b)).build();
                } else if b[..4] == *b"GET " {
                    debug!("  connection is plain http");
                    debug!(
                        "connection is plain http, will pass as is. ta: {:?}, b: {}",
                        params.a,
                        String::from_utf8_lossy(&b)
                    );
                } else {
                    debug!("connection is not tls or plain http, will pass as is");
                }

                let builder = MapResult::new_c(c).a(params.a);

                if b.is_empty() {
                    builder.build()
                } else {
                    builder.b(Some(b)).build()
                }
            }
            ProxyBehavior::ENCODE => {
                // 拿到的是 被 MITM client 脱掉 的 裸链接，这里要把它重新包装一层 tls 连接, 直接使用 direct+ tls/naive_tls 就行

                MapResult::from_err_str(
                    "ENCODE used in MITM map, use Direct with leak_target_addr + TLS instead",
                )
            }
        }
    }
}
