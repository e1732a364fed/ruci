use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{routing::get, Router};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use ruci::net::{GlobalTrafficRecorder, CID};
use ruci::relay::NewConnInfo;
use serde::Serialize;
use tokio::sync::{mpsc, Mutex};
use tracing::info;

pub const DEFAULT_API_ADDR: &str = "127.0.0.1:40681";

type NewConnInfoMap = Arc<RwLock<BTreeMap<CID, (DateTime<Utc>, NewConnInfo)>>>;

/// 缓存 某cid的 某时间点的流量
#[cfg(all(feature = "trace", feature = "api_server"))]
type FluxCache = Arc<tinyufo::TinyUfo<CID, Vec<(tokio::time::Instant, u64)>>>;
#[cfg(all(feature = "trace", feature = "api_server"))]
fn new_cache() -> FluxCache {
    Arc::new(tinyufo::TinyUfo::new(100, 100))
}

#[cfg(all(feature = "trace", feature = "api_server"))]
use std::sync::atomic::AtomicBool;

#[cfg(all(feature = "trace", feature = "api_server"))]
pub struct TracePart {
    pub is_monitoring: Arc<AtomicBool>,

    /// upload info for each conn
    pub u_cache: FluxCache,

    /// download info for each conn
    pub d_cache: FluxCache,
}

pub struct Server {
    listen_addr: Option<String>,

    pub close_engine_tx: mpsc::Sender<()>,

    pub new_conn_info_map: NewConnInfoMap,

    #[cfg(feature = "trace")]
    pub flux_trace: TracePart,
}

impl Server {
    /// non-blocking, init the server and run it
    pub async fn new(
        listen_addr: Option<String>,
        start_core_opts: Opts,
    ) -> (Self, mpsc::Receiver<()>, Arc<GlobalTrafficRecorder>) {
        let (tx, rx) = mpsc::channel(10);

        let global_traffic = Arc::new(GlobalTrafficRecorder::default());

        let server = Server {
            listen_addr,
            close_engine_tx: tx,
            new_conn_info_map: Arc::new(RwLock::new(BTreeMap::new())),

            #[cfg(feature = "trace")]
            flux_trace: TracePart {
                is_monitoring: Arc::new(AtomicBool::new(false)),
                u_cache: new_cache(),
                d_cache: new_cache(),
            },
        };
        serve(&server, global_traffic.clone(), start_core_opts).await;
        (server, rx, global_traffic)
    }
}

#[cfg(feature = "trace")]
async fn is_monitoring_flux(State(is_monitoring_flux): State<Arc<AtomicBool>>) -> String {
    format!("{}", is_monitoring_flux.load(Ordering::Relaxed))
}
#[cfg(feature = "trace")]
async fn enable_monitor(State(is_monitoring_flux): State<Arc<AtomicBool>>) -> &'static str {
    is_monitoring_flux.fetch_or(true, Ordering::Relaxed);
    "ok"
}
#[cfg(feature = "trace")]
async fn disable_monitor(State(is_monitoring_flux): State<Arc<AtomicBool>>) -> &'static str {
    is_monitoring_flux.fetch_and(false, Ordering::Relaxed);
    "ok"
}

async fn get_conn_infos(State(all_conn): State<NewConnInfoMap>) -> String {
    let mut s = String::new();
    let m = all_conn.read();
    for i in m.iter() {
        let x = i.1 .0.to_string();
        s.push_str(&x);
        s.push_str(" , ");
        let x = i.1 .1.to_string();
        s.push_str(&x);
        s.push('\n')
    }
    s
}

async fn get_conn_infos_range(
    Path(cid): Path<String>,
    State(all_conn): State<NewConnInfoMap>,
) -> String {
    use std::str::FromStr;
    let cid = CID::from_str(&cid);
    let cid = match cid {
        Ok(c) => c,
        Err(_) => return String::from("None"),
    };

    let mut s = String::new();
    let m = all_conn.read();
    for i in m.range(cid..) {
        let x = i.1 .0.to_string();
        s.push_str(&x);
        s.push_str(" , ");
        let x = i.1 .1.to_string();
        s.push_str(&x);
        s.push('\n')
    }
    s
}

async fn get_last_ok_cid(State(all_conn): State<NewConnInfoMap>) -> String {
    let mut s = String::new();
    let m = all_conn.read();
    let last_kv = m.last_key_value();
    if let Some(e) = last_kv {
        s.push_str(&e.0.to_string())
    }
    s
}

async fn get_conn_count(State(all_conn): State<NewConnInfoMap>) -> String {
    format!("{}", all_conn.read().len())
}

async fn get_alive_conn_count(State(s): State<Arc<ruci::net::GlobalTrafficRecorder>>) -> String {
    format!("{}", s.alive_connection_count.load(Ordering::Relaxed))
}

async fn get_last_conn_id(State(s): State<Arc<ruci::net::GlobalTrafficRecorder>>) -> String {
    format!("{}", s.last_connection_id.load(Ordering::Relaxed))
}

async fn get_gt_u(State(s): State<Arc<ruci::net::GlobalTrafficRecorder>>) -> String {
    format!("{}", s.ub.load(Ordering::Relaxed))
}

async fn get_gt_d(State(s): State<Arc<ruci::net::GlobalTrafficRecorder>>) -> String {
    format!("{}", s.db.load(Ordering::Relaxed))
}

async fn get_conn_info(Path(cid): Path<String>, State(all_conn): State<NewConnInfoMap>) -> String {
    let mut s = String::new();
    let m = all_conn.read();
    use std::str::FromStr;
    let cid = CID::from_str(&cid);
    let cid = match cid {
        Ok(c) => c,
        Err(_) => return String::from("None"),
    };

    let x = m.get(&cid);
    let i = match x {
        Some(x) => x,
        None => return String::from("None"),
    };
    let x = i.0.to_string();
    s.push_str(&x);
    s.push_str(" , ");
    let x = i.1.to_string();
    s.push_str(&x);
    s
}

#[cfg(feature = "trace")]
async fn get_flux_for(Path(cid): Path<String>, State(cache): State<FluxCache>) -> String {
    use std::str::FromStr;
    let cid = CID::from_str(&cid);
    let cid = match cid {
        Ok(c) => c,
        Err(_) => return String::from("None"),
    };

    let x = cache.get(&cid);

    let x = match x {
        Some(x) => x,
        None => return String::from("None"),
    };

    instant_data_to_str(x)
}

#[cfg(feature = "trace")]
fn instant_data_to_str(v: Vec<(tokio::time::Instant, u64)>) -> String {
    let mut s = String::new();
    for x in v {
        s.push_str("{ -");
        s.push_str(&x.0.elapsed().as_millis().to_string());
        s.push_str(" ms , ");
        s.push_str(&x.1.to_string());
        s.push_str(" },\n");
    }
    s
}

/// stop rucimp core
async fn stop_engine(State(tx): State<mpsc::Sender<()>>) -> String {
    let r = tx.try_send(());
    format!("{:?}", r)
}

pub type Opts = Arc<
    Mutex<
        Option<(
            Server,
            tokio::sync::mpsc::Receiver<()>,
            Arc<ruci::net::GlobalTrafficRecorder>,
        )>,
    >,
>;

async fn start_engine(
    State(api_server_opts): State<Opts>,
    axum::Json(args): axum::Json<crate::modes::CoreArgs>,
) -> String {
    let mut api_server_opts = api_server_opts.lock().await;

    let opts = api_server_opts.as_mut();

    let opts = opts.unwrap();

    let r = crate::modes::init_engine(args, Some(opts)).await;
    match r {
        Ok((mut e, r)) => {
            let id = e.global_data.run_instance_id;
            tokio::spawn(async move { e.run_with_close_rx(r, false).await });
            return id.to_string();
        }
        Err(r) => format!("{:?}", r),
    }
}

#[derive(Serialize)]
struct StatusResponse {
    status: String,
}

pub async fn get_status() -> impl IntoResponse {
    let status = StatusResponse {
        status: "running".to_string(),
    };

    axum::Json(status)
}

/// non-blocking, it calls tokio::spawn
pub async fn serve(
    s: &Server,
    global_traffic: Arc<ruci::net::GlobalTrafficRecorder>,
    start_core_opts: Opts,
) {
    let addr = s
        .listen_addr
        .clone()
        .unwrap_or_else(|| String::from(DEFAULT_API_ADDR));
    info!("api server starting {addr}");

    let mut app = Router::new().route(
        "/stop_engine",
        get(stop_engine).with_state(s.close_engine_tx.clone()),
    );
    app = app
        .route("/status", get(get_status))
        .route(
            "/start_engine",
            post(start_engine).with_state(start_core_opts),
        )
        .route(
            "/gt/acc",
            get(get_alive_conn_count).with_state(global_traffic.clone()),
        )
        .route(
            "/gt/lci",
            get(get_last_conn_id).with_state(global_traffic.clone()),
        )
        .route("/gt/u", get(get_gt_u).with_state(global_traffic.clone()))
        .route("/gt/d", get(get_gt_d).with_state(global_traffic.clone()))
        .route(
            "/all_c",
            get(get_conn_infos).with_state(s.new_conn_info_map.clone()),
        )
        .route(
            "/gt/loci",
            get(get_last_ok_cid).with_state(s.new_conn_info_map.clone()),
        )
        .route(
            "/cr/:cid",
            get(get_conn_infos_range).with_state(s.new_conn_info_map.clone()),
        )
        .route(
            "/cc",
            get(get_conn_count).with_state(s.new_conn_info_map.clone()),
        )
        .route(
            "/c/:cid",
            get(get_conn_info).with_state(s.new_conn_info_map.clone()),
        );

    #[cfg(feature = "trace")]
    {
        let ism = s.flux_trace.is_monitoring.clone();

        app = app.route("/m", get(is_monitoring_flux).with_state(ism.clone()));
        app = app.route("/m_on", get(enable_monitor).with_state(ism.clone()));
        app = app.route("/m_off", get(disable_monitor).with_state(ism.clone()));

        app = app.route(
            "/d/:cid",
            get(get_flux_for).with_state(s.flux_trace.d_cache.clone()),
        );

        app = app.route(
            "/u/:cid",
            get(get_flux_for).with_state(s.flux_trace.u_cache.clone()),
        );
    }

    // RUST_LOG=tower_http=trace

    use axum::http::Method;
    use tower_http::cors::{Any, CorsLayer};
    use tower_http::trace::TraceLayer;

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    tokio::spawn(async move {
        axum::serve(
            listener,
            app.layer(TraceLayer::new_for_http()).layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods([Method::GET, Method::POST])
                    .allow_headers(Any),
            ),
        )
        .await
        .unwrap();
    });

    info!("api server started {addr}");
}

pub async fn setup_api_server_with_chain_engine(
    e: &mut crate::modes::chain::engine::Engine,
    #[cfg(feature = "trace")] is_trace: bool,
    api_ser: &mut Server,
    gtr: Arc<ruci::net::GlobalTrafficRecorder>,
) {
    e.gtr = gtr;

    setup_record_new_conn_info_with_chain_engine(e, api_ser).await;
    #[cfg(feature = "trace")]
    if is_trace {
        setup_trace_flux_for_chain_engine(e, api_ser).await;
    }
}

/// 记录新连接信息
pub async fn setup_record_new_conn_info_with_chain_engine(
    e: &mut crate::modes::chain::engine::Engine,
    api_ser: &mut Server,
) {
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
async fn setup_trace_flux_for_chain_engine(
    se: &mut crate::modes::chain::engine::Engine,
    s: &mut Server,
) {
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
