use map::addr_conn::{AsyncReadAddrExt, AsyncWriteAddrExt, MAX_DATAGRAM_SIZE};

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
            Stream::TCP(_) => todo!(),
            Stream::UDP(mut u) => {
                info!("{cid} udp consumed by echo");

                tokio::spawn(async move {
                    let mut buf = BytesMut::zeroed(MAX_DATAGRAM_SIZE);
                    loop {
                        let r = u.r.read(&mut buf).await;
                        match r {
                            Ok((n, a)) => {
                                let r = u.w.write(&buf[..n], &a).await;
                                if let Err(e) = r {
                                    info!("{cid} echo write stoped by: {e}");

                                    break;
                                }
                            }
                            Err(e) => {
                                info!("{cid} echo read stoped by: {e}");
                                break;
                            }
                        }
                    }
                });
            }
            _ => warn!(
                "{cid} echo needs a single stream to loop read, got {}",
                params.c
            ),
        }

        return MapResult::default();
    }
}
