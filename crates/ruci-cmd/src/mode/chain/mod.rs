use rucimp::{
    modes::chain::{config::StaticConfig, engine::Engine},
    utils::{wait_close_sig, wait_close_sig_with_closer},
};
use tokio::sync::mpsc;
use tracing::info;

#[cfg(feature = "api_server")]
use crate::api;

use std::{sync::Arc, time::Duration};

///blocking
#[allow(unused)]
pub(crate) async fn run(
    mut file_name: String,
    args: crate::Args,
    #[cfg(feature = "api_server")] opts: Option<(
        api::server::Server,
        mpsc::Receiver<()>,
        Arc<ruci::net::GlobalTrafficRecorder>,
    )>,
) -> anyhow::Result<()> {
    info!("starting rucimp chain engine...");

    let mut e = rucimp::modes::chain::engine::Engine::new();

    let (contents, file_source) = crate::mode::get_config_file(&mut file_name, args.in_memory)
        .await
        .context("get_config_file failed")?;

    use anyhow::Context;

    e.file_source = Arc::new(file_source);

    if file_name.ends_with(".lua") {
        #[cfg(any(feature = "lua", feature = "lua54"))]
        {
            if args.infinite {
                e.init_lua_infinite_dynamic(contents)?;
            } else {
                e.init_lua(contents)?;
            }
        }
    } else if file_name.ends_with(".toml") {
        let dr = toml::Deserializer::new(&contents);

        let c: StaticConfig =
            serde_path_to_error::deserialize(dr).context("toml to StaticConfig failed")?;

        e.init_static(c);
    } else if file_name.ends_with(".yml") || file_name.ends_with(".yaml") {
        let dr = serde_yaml::Deserializer::from_str(&contents);

        let c: StaticConfig =
            serde_path_to_error::deserialize(dr).context("yaml to StaticConfig failed")?;

        e.init_static(c);
    } else if file_name.ends_with(".json") {
        let c: StaticConfig =
            rucimp::serde_json::from_str(&contents).context("json to StaticConfig failed")?;
        e.init_static(c);
    } else {
        anyhow::bail!("unsupported file extension: {}", file_name);
    }

    #[cfg(feature = "api_server")]
    {
        if let Some(mut s) = opts {
            setup_api_server_with_chain_engine(
                &mut e,
                #[cfg(feature = "trace")]
                args,
                &mut s.0,
                s.2,
            )
            .await;

            run_engine(&mut e, Some(s.1)).await?;

            return Ok(());
        }
    }

    run_engine(&mut e, None).await?;

    Ok(())
}

/// 阻塞运行Engine, 其运行结束后 会自动对 Engine 调用 reset
async fn run_engine(e: &mut Engine, close_rx: Option<mpsc::Receiver<()>>) -> anyhow::Result<()> {
    use anyhow::Context;
    let mut js = e.run().await.context("run_engine got error")?;

    info!("started rucimp chain engine");

    match close_rx {
        Some(rx) => wait_close_sig_with_closer(rx).await?,
        None => wait_close_sig().await?,
    }

    std::thread::spawn(|| {
        const WAIT_SEC: u64 = 10;
        std::thread::sleep(Duration::from_secs(WAIT_SEC));
        tracing::warn!("Force shutdown after {WAIT_SEC} secs!");
        println!("Force shutdown after {WAIT_SEC} secs!");
        std::process::exit(1);
    });

    e.stop().await;

    js.shutdown().await;

    e.reset().await;
    tracing::info!(
        "chain engine shutted down gracefully, {}",
        e.global_data.run_instance_id
    );

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

    setup_record_new_conn_info(e, api_ser).await;
    #[cfg(feature = "trace")]
    if args.trace {
        setup_trace_flux(e, api_ser).await;
    }
}

/// 记录新连接信息
#[cfg(feature = "api_server")]
async fn setup_record_new_conn_info(e: &mut Engine, api_ser: &mut api::server::Server) {
    let (nci_tx, mut nci_rx) = mpsc::channel(100);

    e.new_conn_recorder = Some(nci_tx);

    let aci = api_ser.new_conn_info_map.clone();

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

    let imc = s.flux_trace.is_monitoring.clone();
    let imc2 = imc.clone();

    let dc = s.flux_trace.d_cache.clone();
    let uc = s.flux_trace.u_cache.clone();

    use ruci::net::CID;
    use std::sync::atomic;
    use tokio::time::Instant;

    fn spawn_for(
        mut rx: mpsc::Receiver<(CID, u64)>,
        is_monitoring: Arc<atomic::AtomicBool>,
        cache: Arc<tinyufo::TinyUfo<CID, Vec<(Instant, u64)>>>,
    ) {
        tokio::spawn(async move {
            loop {
                let x = rx.recv().await;
                match x {
                    Some(info) => {
                        if is_monitoring.load(atomic::Ordering::SeqCst) {
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

    spawn_for(db_rx, imc, dc);
    spawn_for(ub_rx, imc2, uc);
}
