use anyhow::Context;
use rucimp::{
    example_common::{try_get_filecontent, wait_close_sig, wait_close_sig_with_closer},
    modes::chain::{config::lua, engine::Engine},
};
use tokio::sync::mpsc;

use crate::api;

///blocking
pub(crate) async fn run(
    f: &str,
    #[cfg(feature = "api_server")] opts: Option<(api::server::Server, mpsc::Receiver<()>)>,
) -> anyhow::Result<()> {
    let contents = try_get_filecontent("local.lua", Some(f))
        .context(format!("run chain engine try get file {} failed", f))?;

    let mut se = rucimp::modes::chain::engine::Engine::default();
    let sc = lua::load(&contents).expect("has valid lua codes in the file content");

    se.init(sc);

    let mut se = Box::new(se);

    #[cfg(feature = "api_server")]
    {
        if let Some(mut s) = opts {
            setup_api_server_with_chain_engine(&mut se, &mut s.0).await;

            se.run().await?;

            wait_close_sig_with_closer(s.1).await?;

            se.stop().await;

            return Ok(());
        }
    }

    se.run().await?;

    wait_close_sig().await?;

    se.stop().await;

    Ok(())
}

async fn setup_api_server_with_chain_engine(se: &mut Engine, s: &mut api::server::Server) {
    setup_record_newconn_info(se, s).await;
    #[cfg(feature = "trace")]
    setup_trace_flux(se, s).await;
}

async fn setup_record_newconn_info(se: &mut Engine, s: &mut api::server::Server) {
    let (nci_tx, mut nci_rx) = mpsc::channel(100);

    se.newconn_recorder = Some(nci_tx);

    let aci = s.newconn_info.clone();

    tokio::spawn(async move {
        loop {
            let x = nci_rx.recv().await;
            match x {
                Some(nc) => {
                    let mut aci = aci.write();
                    let cid = nc.cid.clone();
                    aci.insert(cid, nc);
                }
                None => break,
            }
        }
    });
}

/// 记录每条连接的实时流量
async fn setup_trace_flux(se: &mut Engine, s: &mut api::server::Server) {
    #[cfg(feature = "trace")]
    {
        let (ub_tx, ub_rx) = mpsc::channel::<(ruci::net::CID, u64)>(4096);

        let (db_tx, db_rx) = mpsc::channel::<(ruci::net::CID, u64)>(4096);

        se.conn_info_updater = Some((ub_tx, db_tx));

        let imcs = s.flux_trace.is_monitoring.clone();
        let imcs2 = imcs.clone();

        let dc = s.flux_trace.d_cache.clone();
        let uc = s.flux_trace.u_cache.clone();

        use ruci::net::CID;
        use std::sync::atomic;
        use std::sync::Arc;
        use tokio::time::Instant;

        fn spawn_for(
            mut rx: mpsc::Receiver<(CID, u64)>,
            is_moniting: Arc<atomic::AtomicBool>,
            cache: Arc<tinyufo::TinyUfo<CID, Vec<(Instant, u64)>>>,
        ) {
            tokio::spawn(async move {
                loop {
                    let x = rx.recv().await;
                    match x {
                        Some(info) => {
                            if is_moniting.load(atomic::Ordering::SeqCst) {
                                let e = (Instant::now(), info.1);

                                let v = cache.get(&info.0);

                                match v {
                                    Some(mut v) => {
                                        v.push(e);
                                        let vl = v.len() as u16;
                                        cache.put(info.0, v, vl);
                                    }
                                    None => {
                                        let v = vec![e];

                                        cache.put(info.0, v, 1);
                                    }
                                }
                            }
                        }
                        None => break,
                    }
                }
            });
        }

        spawn_for(db_rx, imcs, dc);
        spawn_for(ub_rx, imcs2, uc);
    }
}
