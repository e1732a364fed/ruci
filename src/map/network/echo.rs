use map::addr_conn::{AsyncReadAddrExt, AsyncWriteAddrExt, MAX_DATAGRAM_SIZE};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, warn};

use super::*;

/// consumes the stream, loop listen and echo it back.
#[mapper_ext_fields]
#[derive(Clone, Debug, Default, NoMapperExt)]
pub struct Echo {}

impl Name for Echo {
    fn name(&self) -> &'static str {
        "echo"
    }
}

#[async_trait]
impl Mapper for Echo {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::Conn(mut c) => {
                if let Some(b) = params.b {
                    let r = c.write_all(&b).await;
                    if let Err(e) = r {
                        let e = anyhow!("{cid} echo write ed stopped by: {e}");

                        return MapResult::from_e(e);
                    }
                    let r = c.flush().await;
                    if let Err(e) = r {
                        let e = anyhow!("{cid} echo flush ed stopped by: {e}");
                        return MapResult::from_e(e);
                    }
                }

                tokio::spawn(async move {
                    let mut buf = BytesMut::zeroed(MAX_DATAGRAM_SIZE);

                    loop {
                        let r = c.read(&mut buf).await;

                        match r {
                            Ok(n) => {
                                let r = c.write_all(&buf[..n]).await;
                                if let Err(e) = r {
                                    info!(cid = cid.short_str(), " echo write stopped by: {e}");

                                    break;
                                }
                                let r = c.flush().await;
                                if let Err(e) = r {
                                    info!(
                                        cid = cid.short_str(),
                                        " echo write flush stopped by: {e}"
                                    );

                                    break;
                                }
                            }
                            Err(e) => {
                                info!(cid = cid.short_str(), "echo read stoped by: {e}");
                                break;
                            }
                        }
                    }
                });
            }
            Stream::AddrConn(mut u) => {
                if let Some(b) = params.b {
                    if let Some(a) = params.a {
                        debug!(cid = cid.short_str(), "udp echo, write ed {:?}", b.len());

                        let r = u.w.write(&b, &a).await;

                        if let Err(e) = r {
                            let e = anyhow!("{cid} echo write ed stoped by: {e}");

                            return MapResult::from_e(e);
                        }
                    } else {
                        info!(
                            cid = cid.short_str(),
                            " udp echo got earlydata without target_addr, {}",
                            b.len()
                        );
                    }
                }

                tokio::spawn(async move {
                    let mut buf = BytesMut::zeroed(MAX_DATAGRAM_SIZE);
                    loop {
                        //debug!("echo reading");
                        let r = u.r.read(&mut buf).await;

                        match r {
                            Ok((n, a)) => {
                                //debug!("echo read got n, {:?} {}", &buf[..n], a);

                                let r = u.w.write(&buf[..n], &a).await;
                                if let Err(e) = r {
                                    info!(cid = cid.short_str(), "echo write stoped by: {e}");

                                    break;
                                }
                                //debug!("echo write n ok,{}", n);
                            }
                            Err(e) => {
                                info!(cid = cid.short_str(), "echo read stoped by: {e}");
                                break;
                            }
                        }
                    }
                });
            }
            _ => warn!(
                cid = cid.short_str(),
                stream = params.c.to_str(),
                "echo needs a single stream to loop read, got: ",
            ),
        }

        return MapResult::default();
    }
}
