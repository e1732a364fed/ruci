mod folder_serve;

use std::{
    collections::BTreeMap,
    sync::{atomic::Ordering, Arc},
};

#[cfg(feature = "trace")]
use std::sync::atomic::AtomicBool;

use chrono::{DateTime, Utc};

use parking_lot::RwLock;
use ruci::{
    net::{GlobalTrafficRecorder, CID},
    relay::NewConnInfo,
};
#[cfg(feature = "trace")]
use tinyufo::TinyUfo;
use tokio::sync::mpsc;
use tracing::info;

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Command {
    /// start api server
    Run,

    /// serve files in folder "static"
    FileServer,
}

pub async fn deal_args(
    cmd: Command,
    args: &crate::Args,
) -> Option<(Server, mpsc::Receiver<()>, Arc<GlobalTrafficRecorder>)> {
    match cmd {
        Command::Run => return Some(Server::new(args.api_addr.clone()).await),
        Command::FileServer => folder_serve::serve_static(args.file_server_addr.clone()).await,
    }
    None
}

type NewConnInfoMap = Arc<RwLock<BTreeMap<CID, (DateTime<Utc>, NewConnInfo)>>>;

/// 缓存 某cid的 某时间点的流量
#[cfg(feature = "trace")]
type FluxCache = Arc<TinyUfo<CID, Vec<(tokio::time::Instant, u64)>>>;
#[cfg(feature = "trace")]
fn new_cache() -> FluxCache {
    Arc::new(TinyUfo::new(100, 100))
}

#[cfg(feature = "trace")]
pub struct TracePart {
    pub is_monitoring: Arc<AtomicBool>,

    /// upload info for each conn
    pub u_cache: FluxCache,

    /// download info for each conn
    pub d_cache: FluxCache,
}

pub struct Server {
    listen_addr: Option<String>,

    //pub global_traffic: Arc<ruci::net::GlobalTrafficRecorder>,
    pub close_tx: mpsc::Sender<()>,

    pub newconn_info_map: NewConnInfoMap,

    #[cfg(feature = "trace")]
    pub flux_trace: TracePart,
}

impl Server {
    /// non-blocking, init the server and run it
    pub async fn new(
        listen_addr: Option<String>,
    ) -> (Self, mpsc::Receiver<()>, Arc<GlobalTrafficRecorder>) {
        let (tx, rx) = mpsc::channel(10);
        let s = Server {
            listen_addr,
            close_tx: tx,
            newconn_info_map: Arc::new(RwLock::new(BTreeMap::new())),

            #[cfg(feature = "trace")]
            flux_trace: TracePart {
                is_monitoring: Arc::new(AtomicBool::new(false)),
                u_cache: new_cache(),
                d_cache: new_cache(),
            },
        };
        let global_traffic = Arc::new(GlobalTrafficRecorder::default());
        serve(&s, global_traffic.clone()).await;
        (s, rx, global_traffic)
    }
}

use axum::extract::{Path, State};
use axum::{routing::get, Router};

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

async fn get_conn_infos(State(allconn): State<NewConnInfoMap>) -> String {
    let mut s = String::new();
    let m = allconn.read();
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
    State(allconn): State<NewConnInfoMap>,
) -> String {
    use std::str::FromStr;
    let cid = CID::from_str(&cid);
    let cid = match cid {
        Ok(c) => c,
        Err(_) => return String::from("None"),
    };

    let mut s = String::new();
    let m = allconn.read();
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

async fn get_last_ok_cid(State(allconn): State<NewConnInfoMap>) -> String {
    let mut s = String::new();
    let m = allconn.read();
    let lastkv = m.last_key_value();
    match lastkv {
        Some(e) => s.push_str(&e.0.to_string()),
        None => {}
    }
    s
}

async fn get_conn_count(State(allconn): State<NewConnInfoMap>) -> String {
    format!("{}", allconn.read().len())
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

async fn get_conn_info(Path(cid): Path<String>, State(allconn): State<NewConnInfoMap>) -> String {
    let mut s = String::new();
    let m = allconn.read();
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

    instant_data_tostr(x)
}

#[cfg(feature = "trace")]
fn instant_data_tostr(v: Vec<(tokio::time::Instant, u64)>) -> String {
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
async fn stop_core(State(tx): State<mpsc::Sender<()>>) -> String {
    let r = tx.try_send(());
    format!("{:?}", r)
}

/// non-blocking
pub async fn serve(s: &Server, global_traffic: Arc<ruci::net::GlobalTrafficRecorder>) {
    let addr = s
        .listen_addr
        .clone()
        .unwrap_or_else(|| String::from(DEFAULT_API_ADDR));
    info!("api server starting {addr}");

    let mut app = Router::new().route("/stop_core", get(stop_core).with_state(s.close_tx.clone()));
    app = app
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
            "/allc",
            get(get_conn_infos).with_state(s.newconn_info_map.clone()),
        )
        .route(
            "/gt/loci",
            get(get_last_ok_cid).with_state(s.newconn_info_map.clone()),
        )
        .route(
            "/cr/:cid",
            get(get_conn_infos_range).with_state(s.newconn_info_map.clone()),
        )
        .route(
            "/cc",
            get(get_conn_count).with_state(s.newconn_info_map.clone()),
        )
        .route(
            "/c/:cid",
            get(get_conn_info).with_state(s.newconn_info_map.clone()),
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
                    .allow_methods([Method::GET]),
            ),
        )
        .await
        .unwrap();
    });

    info!("api server started {addr}");
}
