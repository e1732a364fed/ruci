use crate::{
    map::{self},
    net::listen::Listener,
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
) -> Receiver<MapResult> {
    let (tx, rx) = mpsc::channel(100);

    tokio::spawn(async move {
        let laddr = listener.laddr();
        tokio::select! {
            r = real_loop_accept(listener,tx) =>{
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
pub async fn loop_accept_forever(listener: Listener) -> Receiver<MapResult> {
    let (tx, rx) = mpsc::channel(100);

    tokio::spawn(real_loop_accept(listener, tx));

    rx
}

/// blocking
async fn real_loop_accept(listener: Listener, tx: Sender<MapResult>) -> anyhow::Result<()> {
    let lastr;

    loop {
        let r = listener.accept().await;

        let (stream, raddr, laddr) = match r {
            Ok(x) => x,
            Err(e) => {
                let e = anyhow!("listen tcp ended by listen e: {}", e);
                info!("{}", e);
                lastr = Err(e);
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
            .send(MapResult::builder().c(stream).d(Some(output_data)).build())
            .await;

        if let Err(e) = r {
            let e = anyhow!("listen tcp ended by tx e: {}", e);
            info!("{}", e);
            lastr = Err(e);
            break;
        }
    }
    lastr
}
