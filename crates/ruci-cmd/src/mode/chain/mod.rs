use rucimp::{
    modes::chain::engine::Engine,
    utils::{wait_close_sig, wait_close_sig_with_closer},
};
use tokio::sync::mpsc;
use tracing::info;

#[cfg(feature = "api_server")]
use crate::api;

#[cfg(feature = "api_server")]
use std::sync::Arc;

///blocking
#[allow(unused)]
pub(crate) async fn run(
    f: &str,
    args: crate::Args,
    #[cfg(feature = "api_server")] opts: Option<(
        api::server::Server,
        mpsc::Receiver<()>,
        Arc<ruci::net::GlobalTrafficRecorder>,
    )>,
) -> anyhow::Result<()> {
    info!("try to start rucimp chain engine");

    let mut se = rucimp::modes::chain::engine::Engine::default();

    #[cfg(feature = "lua")]
    {
        use anyhow::Context;

        let contents = rucimp::utils::try_get_filecontent("local.lua", Some(f))
            .with_context(|| format!("run chain engine try get file {} failed", f))?;

        if args.infinite {
            se.init_lua_infinite_dynamic(contents)?;
        } else {
            se.init_lua(contents)?;
        }
    }

    #[cfg(feature = "api_server")]
    {
        if let Some(mut s) = opts {
            setup_api_server_with_chain_engine(
                &mut se,
                #[cfg(feature = "trace")]
                args,
                &mut s.0,
                s.2,
            )
            .await;

            run_engine(&se, Some(s.1)).await?;

            return Ok(());
        }
    }

    run_engine(&se, None).await?;

    Ok(())
}

async fn run_engine(e: &Engine, close_rx: Option<mpsc::Receiver<()>>) -> anyhow::Result<()> {
    let mut js = e.run().await?;

    info!("started rucimp chain engine");

    match close_rx {
        Some(rx) => wait_close_sig_with_closer(rx).await?,
        None => wait_close_sig().await?,
    }

    e.stop().await;

    js.shutdown().await;
    Ok(())
}

#[cfg(feature = "api_server")]
async fn setup_api_server_with_chain_engine(
    e: &mut Engine,
    #[cfg(feature = "trace")] args: crate::Args,
    api_ser: &mut api::server::Server,
    gtr: Arc<ruci::net::GlobalTrafficRecorder>,
) {
    e.gtr = gtr;

    setup_record_newconn_info(e, api_ser).await;
    #[cfg(feature = "trace")]
    if args.trace {
        setup_trace_flux(e, api_ser).await;
    }
}

/// 记录新连接信息
#[cfg(feature = "api_server")]
async fn setup_record_newconn_info(e: &mut Engine, api_ser: &mut api::server::Server) {
    let (nci_tx, mut nci_rx) = mpsc::channel(100);

    e.newconn_recorder = Some(nci_tx);

    let aci = api_ser.newconn_info_map.clone();

    tokio::spawn(async move {
        loop {
            let x = nci_rx.recv().await;
            match x {
                Some(nc) => {
                    let mut aci = aci.write();
                    let cid = nc.cid.clone();

                    use chrono::Utc;
                    let now: chrono::DateTime<Utc> = Utc::now();
                    aci.insert(cid, (now, nc));
                }
                None => break,
            }
        }
    });
}

/// 记录每条连接的实时流量
#[cfg(feature = "trace")]
#[cfg(feature = "api_server")]
async fn setup_trace_flux(se: &mut Engine, s: &mut api::server::Server) {
    let (ub_tx, ub_rx) = mpsc::channel::<(ruci::net::CID, u64)>(4096);

    let (db_tx, db_rx) = mpsc::channel::<(ruci::net::CID, u64)>(4096);

    se.conn_info_updater = Some((ub_tx, db_tx));

    let imcs = s.flux_trace.is_monitoring.clone();
    let imcs2 = imcs.clone();

    let dc = s.flux_trace.d_cache.clone();
    let uc = s.flux_trace.u_cache.clone();

    use ruci::net::CID;
    use std::sync::atomic;
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
