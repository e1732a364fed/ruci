use crate::{map::AnyData, net::listen::Listener};
use anyhow::anyhow;
use log::{debug, info, log_enabled};
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    oneshot,
};

use super::MapResult;

/// non-blocking
pub async fn loop_accept(
    listener: Listener,
    shutdown_rx: oneshot::Receiver<()>,
) -> Receiver<MapResult> {
    let (tx, rx) = mpsc::channel(100);

    let netw = listener.network();

    tokio::spawn(async move {
        tokio::select! {
            r = real_loop_accept(listener,tx) =>{
                r
            }
            _ = shutdown_rx => {
                info!("terminating {} listen", netw);
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
        if log_enabled!(log::Level::Debug) {
            debug!(
                "new accepted {}, raddr: {}, laddr: {}",
                listener.network(),
                raddr,
                laddr
            );
        }

        let r = tx
            .send(
                MapResult::builder()
                    .c(stream)
                    .d(AnyData::RLAddr((raddr, laddr)))
                    .build(),
            )
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
