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

    let (contents, data_source) = get_config_file(&mut file_name, args.in_memory)
        .await
        .context("get_config_file failed")?;

    use anyhow::Context;

    e.data_source = Arc::new(data_source);

    if file_name.ends_with(".lua") {
        #[cfg(any(feature = "lua", feature = "lua54"))]
        {
            if args.infinite {
                e.init_lua_infinite_dynamic(contents)?;
            } else {
                e.init_lua(contents)?;
            }
        }
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

use std::path::Path;

use anyhow::bail;
use data_source::{DataSource, SyncFolderSource};
use rucimp::DEFAULT_LUA_CONFIG_FILE_NAME;
use tracing::debug;

pub async fn get_config_file(
    file_name: &mut String,
    in_memory: bool,
) -> anyhow::Result<(String, DataSource)> {
    use anyhow::Context;

    let mut data_source = rucimp::utils::default_file_source();
    data_source
        .insert_current_working_dir()
        .context("insert_current_working_dir failed")?;

    let get_file_f = || -> anyhow::Result<_> {
        let mut r = data_source.get_file_content(Path::new(&file_name));

        if r.is_err() {
            r = data_source.get_file_content(Path::new(DEFAULT_LUA_CONFIG_FILE_NAME));
        }

        r.context("get_config_file data_source.get_file_content failed")
    };

    //获取到文件的 bytes, 或通过下载 或读取文件. 若 in_memory 给出则下载的文件不持久化

    let (mut file_bytes_v, _found_dir) =
        if file_name.starts_with("http://") || file_name.starts_with("https://") {
            #[cfg(feature = "utils")]
            {
                use std::io::Read;

                let url: String = file_name.to_string();

                file_name.replace_range(.., url.split('/').last().unwrap());

                match in_memory {
                    true => (crate::utils::dl_url(&url, None).await?.unwrap(), None),
                    false => {
                        let _ = crate::utils::dl_url(&url, Some(file_name)).await?;

                        let mut v = vec![];

                        let mut file = std::fs::File::open(file_name.clone())?;
                        file.read_to_end(&mut v)?;

                        (v, None)
                    }
                }
            }

            #[cfg(not(feature = "utils"))]
            {
                get_file_f()?
            }
        } else {
            get_file_f()?
        };

    // zip, tar, lua/toml 三种情况. zip 要解压
    // 之后若为 tar, 则会将 Engine 的 DataSource 设为 该tar, 后续 Engine 访问文件都会只在该tar 中寻找

    if file_name.to_lowercase().ends_with(".zip") {
        let real_fn = file_name.strip_suffix(".zip").unwrap_or(file_name);

        file_bytes_v = rucimp::utils::extract_vec_from_zip(real_fn, file_bytes_v)?;
        *file_name = real_fn.to_string();
    }

    if file_name.to_lowercase().ends_with(".tar") {
        let tar_file_bytes_v = file_bytes_v;
        let md5_s = format!(
            "{:x}",
            rucimp::utils::md5::compute(tar_file_bytes_v.as_slice())
        );

        let should_be = file_name.split_once('.').unwrap().0;

        if should_be != md5_s {
            bail!(
                "md5 do not match: should be {}, but got {}",
                should_be,
                md5_s
            );
        } else {
            debug!("md5 match")
        }

        use data_source::get_file_from_tar_in_memory;

        //在 tar 的情况下，约定所使用的 配置文件 名称只能为 local.lua或 local.json
        let mut real_file_bytes_r =
            get_file_from_tar_in_memory(DEFAULT_LUA_CONFIG_FILE_NAME, &tar_file_bytes_v);

        if real_file_bytes_r.is_err() {
            real_file_bytes_r = get_file_from_tar_in_memory("local.json", &tar_file_bytes_v);
        }

        let (real_file_bytes, _) = real_file_bytes_r.context("get_file_from_tar failed")?;

        data_source = DataSource::TarInMemory(tar_file_bytes_v);
        file_bytes_v = real_file_bytes;

        let real_fn = file_name.strip_suffix(".tar").unwrap_or(file_name);
        *file_name = real_fn.to_string();
    }

    let contents = String::from_utf8_lossy(file_bytes_v.as_slice()).to_string();

    Ok((contents, data_source))
}
#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use ruci::net::dns::{self};
    use rucimp::modes::chain::config::PlainTextPassSet;
    use rucimp::modes::chain::config::{DirectConfig, InMapConfig, OutMapConfig, StaticConfig};
    use rucimp::serde_json;

    #[test]
    fn serialize() {
        let sa = std::net::SocketAddr::V4("114.114.114.114:53".parse().unwrap());

        let c1 = vec![
            InMapConfig::Listener {
                listen_addr: "0.0.0.0:1080".to_string(),
                ext: None,
            },
            InMapConfig::Counter,
            InMapConfig::Adder { value: 3 },
            InMapConfig::Socks5(PlainTextPassSet::default()),
        ];
        let mut inbounds = BTreeMap::new();
        inbounds.insert("c1".to_string(), c1);

        let c2 = vec![
            OutMapConfig::Direct(DirectConfig::default()),
            OutMapConfig::Direct(DirectConfig {
                dns_client: Some(dns::ClientConfig {
                    dns_server_list: vec![(sa, dns::TheProtocol::Udp)],
                    ip_strategy: Some(dns::TheLookupIpStrategy::Ipv4Only),
                    static_pairs: None,
                }),
                ..Default::default()
            }),
        ];

        let mut outbounds = BTreeMap::new();
        outbounds.insert("c2".to_string(), c2);

        let sc = StaticConfig {
            inbounds,
            outbounds,
            ..Default::default()
        };
        let json = serde_json::to_string(&sc).expect("valid json");
        println!("{:#}", json);

        let sc: StaticConfig = serde_json::from_str(&json).expect("valid json");
        println!("{:#?}", sc);
    }
}
