use crate::{map::AnyData, net::listen::Listener};
use anyhow::anyhow;
use log::{debug, info};
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    oneshot,
};

use super::MapResult;

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

async fn real_loop_accept(listener: Listener, tx: Sender<MapResult>) -> anyhow::Result<()> {
    let lastr;

    loop {
        let r = listener.accept().await;

        let (stream, raddr) = match r {
            Ok(x) => x,
            Err(e) => {
                let e = anyhow!("listen tcp ended by listen e: {}", e);
                info!("{}", e);
                lastr = Err(e);
                break;
            }
        };

        debug!("new accepted {}, raddr: {}", listener.network(), raddr);

        let r = tx
            .send(
                MapResult::builder()
                    .c(stream)
                    .d(AnyData::Addr(raddr))
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
