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
use ruci::utils::ob_to_buf;
use ruci::{map, net::MTU};
use std::fmt::Display;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tracing::{debug, info};

#[map_ext_fields]
#[derive(Debug, Clone, MapExt)]
pub struct MITM {
    pub sc: crate::server::ServerPEMOptions,
}

impl Display for MITM {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "mitm_server")
    }
}

pub enum DataType {
    //host
    TlsClientHello(String),
    Other,
}

pub fn check_data_type(b: &[u8]) -> DataType {
    if b.len() < 4 {
        return DataType::Other;
    }
    if b[..2] == *b"\x16\x03" {
        let host = crate::extract_host_from_client_hello(&b);
        match host {
            Some(host) => return DataType::TlsClientHello(host),
            None => return DataType::Other,
        }
    }

    DataType::Other
}

pub enum CheckResult {
    //host, unwrapped stream
    TlsClientHello(String, ruci::net::Conn),
    //other data, original conn
    Other(BytesMut, ruci::net::Conn),
}

impl MITM {
    pub async fn check(&self, b: BytesMut, c: ruci::net::Conn) -> anyhow::Result<CheckResult> {
        let data_type = check_data_type(&b);

        if let DataType::TlsClientHello(host) = &data_type {
            let authority = host;
            let sc = crate::load::load_ser_config_from_pem(
                &self.sc,
                Some(&Authority::from_maybe_shared(authority.clone()).unwrap()),
            )
            .unwrap();

            let ec = EarlyDataWrapper::from(b, c);

            let stream = match tokio_rustls::TlsAcceptor::from(Arc::new(sc))
                .accept(ec)
                .await
            {
                Ok(stream) => Box::new(stream),
                Err(e) => {
                    let e = anyhow!("MITM: Failed to establish TLS connection: {}", e);
                    return Err(e);
                }
            };

            // let alpn = { tls::extract_alpn_from_clinet_hello(&b) };
            // debug!("MITM: client shown alpn is {:?}", alpn);

            Ok(CheckResult::TlsClientHello(host.clone(), stream))
        } else {
            Ok(CheckResult::Other(b, c))
        }
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

                let b = ob_to_buf(params.b);

                let b = if !b.is_empty() {
                    b
                } else {
                    let mut b = BytesMut::zeroed(MTU);

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
                                b.truncate(n);
                                b
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
                let r = self.check(b, c).await;

                if let Err(e) = r {
                    return MapResult::from_e(e);
                }

                let check_result = r.unwrap();

                match check_result {
                    CheckResult::TlsClientHello(authority, mut stream) => {
                        // params.a and authority should be the same

                        let ta = if let Some(a) = params.a {
                            debug!("MITM: ta: {:?}, authority: {}", a, authority);

                            //http 代理时，ta 直接是 authority;
                            // socks5 代理时，ta 可能是 目标的tcp 地址 （如果没有 配置 使用 socks5 解析dns)

                            // if a.get_name().unwrap() != authority {
                            //     let e = anyhow!(
                            //         "MITM: authority not match, ta: {:?}, authority: {}",
                            //         a,
                            //         authority
                            //     );
                            //     error!("{}", e);

                            //     std::process::exit(1);
                            // } else {
                            //     debug!(
                            //         "MITM TLS connection established, ta: {:?}, authority: {}",
                            //         a, authority
                            //     );
                            // }

                            Some(a)
                        } else {
                            debug!(
                                "MITM TLS connection established, no ta given, authority: {}",
                                authority
                            );

                            Some(
                                ruci::net::Addr::from_addr_str("tcp", &(authority + ":443"))
                                    .unwrap(),
                            )
                        };

                        // 重新读取一次，得到 tls 的首包用户数据

                        let mut b = BytesMut::zeroed(MTU * 2);
                        match stream.read(&mut b).await {
                            Ok(n) => {
                                debug!(
                                    "MITM: first read client data success, len: {}, content: {}",
                                    n,
                                    b[..n.min(100)].escape_ascii()
                                );
                                b.truncate(n);

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
                            }
                            Err(e) => {
                                return MapResult::from_e(anyhow!(
                                    "MITM: first read client data failed: {}",
                                    e
                                ))
                            }
                        }

                        return MapResult::new_c(stream).a(ta).b(Some(b)).build();
                    }

                    CheckResult::Other(b, c) => {
                        debug!("connection is not tls, will pass as is");

                        //     debug!(
                        //         "connection data. ta: {:?}, b: {}",
                        //         params.a,
                        //         String::from_utf8_lossy(&b)
                        //     );

                        let builder = MapResult::new_c(c).a(params.a);

                        if b.is_empty() {
                            builder.build()
                        } else {
                            builder.b(Some(b)).build()
                        }
                    }
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
