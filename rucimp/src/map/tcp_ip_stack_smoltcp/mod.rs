pub mod device;
pub mod ip_packet;
pub mod tcp;
pub mod udp;

use async_trait::async_trait;
use ruci::map::{self, *};
use ruci::net::*;
use ruci::Name;

use macro_map::*;
use tokio::sync::mpsc;
use tracing::debug;

use self::device::SmoltcpDevice;

/// split the incomming ip stream into multiple tcp/udp stream
#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct Stack {}

impl Name for Stack {
    fn name(&self) -> &'static str {
        "smoltcp"
    }
}

#[async_trait]
impl Map for Stack {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::Conn(c) => {
                let mut shutdown_rx = match params.shutdown_rx {
                    Some(r) => r,
                    None => {
                        return MapResult::err_str(
                            "smoltcp requires a shutdown_rx for graceful shutdown",
                        )
                    }
                };

                let (new_stream_tx, new_stream_rx) = mpsc::channel(1000);

                tokio::spawn(async move {
                    let mut device = SmoltcpDevice::new(cid, c, new_stream_tx);

                    let mut iface = device::create_interface(&mut device);

                    loop {
                        tokio::select! {
                            _ = &mut shutdown_rx =>{
                                debug!("smoltcp got shutdown signal");
                                break;
                            }
                            r = device.read() =>{
                                match r {
                                    Ok(_) => {
                                        let sockets = &mut device.sockets as *mut smoltcp::iface::SocketSet;

                                        iface.poll(smoltcp::time::Instant::now(),&mut device, unsafe {
                                            &mut *sockets
                                        });
                                    },
                                    Err(e) => {
                                        tracing::warn!("SmoltcpDevice read got e {e}");
                                        break;
                                    },
                                }
                            }

                        }
                    }
                });

                return MapResult::builder()
                    .a(params.a)
                    .b(params.b)
                    .c(ruci::net::Stream::Generator(new_stream_rx))
                    .build();
            }
            _ => {
                return MapResult::err_str(&format!(
                    "smoltcp only support Conn stream, got {}",
                    params.c
                ))
            }
        }
    }
}
