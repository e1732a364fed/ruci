mod folder_serve;

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use parking_lot::RwLock;
use ruci::{net::CID, relay::NewConnInfo};
use tinyufo::TinyUfo;

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Commands {
    /// start api server
    Run,

    /// serve files in folder "static"
    FileServer,
}

pub async fn deal_cmds(cmd: Commands) -> Option<Server> {
    match cmd {
        Commands::Run => return Some(Server::new().await),
        Commands::FileServer => folder_serve::serve_static().await,
    }
    None
}

type NewConnInfoMap = Arc<RwLock<BTreeMap<CID, NewConnInfo>>>;

pub struct Server {
    pub newconn_info: NewConnInfoMap,

    #[cfg(feature = "trace")]
    pub flux_trace: TracePart,
}

/// 缓存 某时间点的流量
type FluxCache = Arc<TinyUfo<CID, Vec<(tokio::time::Instant, u64)>>>;
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

impl Server {
    /// non-blocking, init the server and run it
    pub async fn new() -> Self {
        let s = Server {
            newconn_info: Arc::new(RwLock::new(BTreeMap::new())),

            #[cfg(feature = "trace")]
            flux_trace: TracePart {
                is_monitoring: Arc::new(AtomicBool::new(false)),
                u_cache: new_cache(),
                d_cache: new_cache(),
            },
        };
        serve(&s).await;
        s
    }
}

use axum::extract::{Path, State};
use axum::{routing::get, Router};

async fn is_monitoring_flux(State(is_monitoring_flux): State<Arc<AtomicBool>>) -> String {
    format!("{}", is_monitoring_flux.load(Ordering::Relaxed))
}

async fn enable_monitor(State(is_monitoring_flux): State<Arc<AtomicBool>>) -> &'static str {
    is_monitoring_flux.fetch_or(true, Ordering::Relaxed);
    "ok"
}

async fn disable_monitor(State(is_monitoring_flux): State<Arc<AtomicBool>>) -> &'static str {
    is_monitoring_flux.fetch_and(false, Ordering::Relaxed);
    "ok"
}

async fn get_conn_infos(State(allconn): State<NewConnInfoMap>) -> String {
    let mut s = String::new();
    let m = allconn.read();
    for i in m.iter() {
        let x = i.1.to_string();
        s.push_str(&x);
        s.push('\n')
    }
    s
}

async fn get_conn_info(Path(cid): Path<String>, State(allconn): State<NewConnInfoMap>) -> String {
    let mut s = String::new();
    let m = allconn.read();
    let cid = CID::from_str(&cid);
    let cid = match cid {
        Some(c) => c,
        None => return String::from("None"),
    };

    let x = m.get(&cid);
    let x = match x {
        Some(x) => x,
        None => return String::from("None"),
    };
    let x = x.to_string();
    s.push_str(&x);
    s
}

async fn get_flux_for(Path(cid): Path<String>, State(cache): State<FluxCache>) -> String {
    let cid = CID::from_str(&cid);
    let cid = match cid {
        Some(c) => c,
        None => return String::from("None"),
    };

    let x = cache.get(&cid);

    let x = match x {
        Some(x) => x,
        None => return String::from("None"),
    };

    instant_data_tostr(x)
}

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

/// non-blocking
pub async fn serve(s: &Server) {
    let addr = "0.0.0.0:3000";
    info!("api server starting {addr}");

    let mut app = Router::new();

    app = app
        .route("/c", get(get_conn_infos).with_state(s.newconn_info.clone()))
        .route(
            "/c/:cid",
            get(get_conn_info).with_state(s.newconn_info.clone()),
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

    use tower_http::trace::TraceLayer;
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app.layer(TraceLayer::new_for_http()))
            .await
            .unwrap();
    });

    info!("server started {addr}");
}
