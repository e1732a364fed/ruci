use crate::{
    map,
    net::{self, listen::Listener},
};
use anyhow::anyhow;
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    oneshot,
};
use tracing::{debug, info};

use super::MapResult;

/// non-blocking
pub async fn loop_accept(
    listener: Listener,
    shutdown_rx: oneshot::Receiver<()>,
    opt_fixed_target_addr: Option<net::Addr>,
) -> Receiver<MapResult> {
    let (tx, rx) = mpsc::channel(100);

    tokio::spawn(async move {
        let laddr = listener.laddr();
        tokio::select! {
            r = real_loop_accept(listener,tx,opt_fixed_target_addr) =>{
                r
            }
            _ = shutdown_rx => {
                info!(laddr=laddr, "terminating listen");
                Ok(())
            }
        }
    });
    rx
}

/// non-blocking
pub async fn loop_accept_forever(
    listener: Listener,
    opt_fixed_target_addr: Option<net::Addr>,
) -> Receiver<MapResult> {
    let (tx, rx) = mpsc::channel(100);

    tokio::spawn(real_loop_accept(listener, tx, opt_fixed_target_addr));

    rx
}

/// blocking
async fn real_loop_accept(
    listener: Listener,
    tx: Sender<MapResult>,
    opt_fixed_target_addr: Option<net::Addr>,
) -> anyhow::Result<()> {
    let last_r;

    loop {
        let r = listener.accept().await;

        let (stream, raddr, laddr) = match r {
            Ok(x) => x,
            Err(e) => {
                let e = anyhow!("listen tcp ended by listen e: {}", e);
                info!("{}", e);
                last_r = Err(e);
                break;
            }
        };
        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!(
                net = %listener.network(),
                raddr = %raddr,
                laddr = %laddr,
                "new accepted",
            );
        }
        let data = map::RLAddr(raddr, laddr);
        let output_data = Box::new(data);

        let r = tx
            .send(
                MapResult::builder()
                    .a(opt_fixed_target_addr.clone())
                    .c(stream)
                    .d(Some(output_data))
                    .build(),
            )
            .await;

        if let Err(e) = r {
            let e = anyhow!("listen tcp ended by tx e: {}", e);
            info!("{}", e);
            last_r = Err(e);
            break;
        }
    }
    last_r
}
