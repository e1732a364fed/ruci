use std::collections::{BTreeMap, HashMap};
use std::env::current_dir;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::bail;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{routing::get, Router};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use ruci::net::{GlobalTrafficRecorder, CID};
use ruci::relay::NewConnInfo;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info};
use utoipa_swagger_ui::SwaggerUi;

type NewConnInfoMap = Arc<RwLock<BTreeMap<CID, (DateTime<Utc>, NewConnInfo)>>>;

/// 缓存 某cid的 某时间点的流量
#[cfg(feature = "trace")]
type FluxCache = Arc<tinyufo::TinyUfo<CID, Vec<(tokio::time::Instant, u64)>>>;
#[cfg(feature = "trace")]
fn new_cache() -> FluxCache {
    Arc::new(tinyufo::TinyUfo::new(100, 100))
}

#[cfg(feature = "trace")]
use std::sync::atomic::AtomicBool;

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

    pub close_engine_tx: mpsc::Sender<()>,

    pub new_conn_info_map: NewConnInfoMap,

    #[cfg(feature = "trace")]
    pub flux_trace: TracePart,

    pub api_extensions: ApiExtensionMap,
}

pub type ApiExtensionMap = Arc<RwLock<HashMap<String, axum::routing::MethodRouter>>>;

impl Server {
    /// non-blocking, init the server and run it
    pub async fn new(
        listen_addr: Option<String>,
        start_core_opts: Opts,
        api_extensions: Option<ApiExtensionMap>,
        extension_api_doc: Option<utoipa::openapi::OpenApi>,
        #[cfg(feature = "file_server")] file_server_tar_data_source_base64: Option<String>,
    ) -> anyhow::Result<(Self, mpsc::Receiver<()>, Arc<GlobalTrafficRecorder>)> {
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

            api_extensions: api_extensions.unwrap_or_else(|| Arc::new(RwLock::new(HashMap::new()))),
        };
        serve(
            &server,
            global_traffic.clone(),
            start_core_opts,
            extension_api_doc,
            #[cfg(feature = "file_server")]
            file_server_tar_data_source_base64,
        )
        .await?;
        Ok((server, rx, global_traffic))
    }
}

#[cfg(feature = "trace")]
#[utoipa::path(
    get,
    path = "/api/monitoring/status",
    tag = "ruci",
    responses(
        (status = 200, description = "Get monitoring status", body = String)
    )
)]
async fn is_monitoring_flux(State(is_monitoring_flux): State<Arc<AtomicBool>>) -> String {
    format!("{}", is_monitoring_flux.load(Ordering::Relaxed))
}

#[cfg(feature = "trace")]
#[utoipa::path(
    get,
    path = "/api/monitoring/enable",
    tag = "ruci",
    responses(
        (status = 200, description = "Enable monitoring", body = String)
    )
)]
async fn enable_monitor(State(is_monitoring_flux): State<Arc<AtomicBool>>) -> &'static str {
    is_monitoring_flux.fetch_or(true, Ordering::Relaxed);
    "ok"
}

#[cfg(feature = "trace")]
#[utoipa::path(
    get,
    path = "/api/monitoring/disable",
    tag = "ruci",
    responses(
        (status = 200, description = "Disable monitoring", body = String)
    )
)]
async fn disable_monitor(State(is_monitoring_flux): State<Arc<AtomicBool>>) -> &'static str {
    is_monitoring_flux.fetch_and(false, Ordering::Relaxed);
    "ok"
}

#[utoipa::path(
    get,
    path = "/api/connections",
    tag = "ruci",
    responses(
        (status = 200, description = "Get all connection information", body = String)
    )
)]

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

#[utoipa::path(
    get,
    path = "/api/connections/range/{cid}",
    tag = "ruci",
    params(
        ("cid" = String, Path, description = "Connection ID to start range from")
    ),
    responses(
        (status = 200, description = "Get connection information from specified CID onwards", body = String)
    )
)]
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

#[utoipa::path(
    get,
    path = "/api/connections/last/ok",
    tag = "ruci",
    responses(
        (status = 200, description = "Get last successful connection ID", body = String)
    )
)]
async fn get_last_ok_cid(State(all_conn): State<NewConnInfoMap>) -> String {
    let mut s = String::new();
    let m = all_conn.read();
    let last_kv = m.last_key_value();
    if let Some(e) = last_kv {
        s.push_str(&e.0.to_string())
    }
    s
}

#[utoipa::path(
    get,
    path = "/api/connections/count",
    tag = "ruci",
    responses(
        (status = 200, description = "Get total connection count", body = String)
    )
)]
async fn get_conn_count(State(all_conn): State<NewConnInfoMap>) -> String {
    format!("{}", all_conn.read().len())
}

#[utoipa::path(
    get,
    path = "/api/traffic/connections/alive/count",
    tag = "ruci",
    responses(
        (status = 200, description = "Get alive connection count", body = String)
    )
)]

async fn get_alive_conn_count(State(s): State<Arc<ruci::net::GlobalTrafficRecorder>>) -> String {
    format!("{}", s.alive_connection_count.load(Ordering::Relaxed))
}

#[utoipa::path(
    get,
    path = "/api/traffic/connections/last/id",
    tag = "ruci",
    responses(
        (status = 200, description = "Get last connection ID", body = String)
    )
)]
async fn get_last_conn_id(State(s): State<Arc<ruci::net::GlobalTrafficRecorder>>) -> String {
    format!("{}", s.last_connection_id.load(Ordering::Relaxed))
}

#[utoipa::path(
    get,
    path = "/api/traffic/upload",
    tag = "ruci",
    responses(
        (status = 200, description = "Get upload traffic statistics", body = String)
    )
)]
async fn get_gt_u(State(s): State<Arc<ruci::net::GlobalTrafficRecorder>>) -> String {
    format!("{}", s.ub.load(Ordering::Relaxed))
}

#[utoipa::path(
    get,
    path = "/api/traffic/download",
    tag = "ruci",
    responses(
        (status = 200, description = "Get download traffic statistics", body = String)
    )
)]
async fn get_gt_d(State(s): State<Arc<ruci::net::GlobalTrafficRecorder>>) -> String {
    format!("{}", s.db.load(Ordering::Relaxed))
}

#[utoipa::path(
    get,
    path = "/api/connections/{cid}",
    tag = "ruci",
    params(
        ("cid" = String, Path, description = "Connection ID")
    ),
    responses(
        (status = 200, description = "Get information for a specific connection", body = String)
    )
)]
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

#[utoipa::path(
    get,
    path = "/api/engine/stop",
    tag = "ruci",
    responses(
        (status = 200, description = "Stop the engine", body = String)
    )
)]
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

#[utoipa::path(
    post,
    path = "/api/engine/start",
    tag = "ruci",
    request_body = crate::modes::CoreArgs,
    responses(
        (status = 200, description = "Start the engine", body = String)
    )
)]
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
            id.to_string()
        }
        Err(r) => format!("{:?}", Result::<(), anyhow::Error>::Err(r)),
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "api_server", derive(utoipa::ToSchema))]

pub struct StatusResponse {
    /// Current status of the server
    pub status: String,
}

#[utoipa::path(
    get,
    path = "/api/status",
    tag = "ruci",
    responses(
        (status = 200, description = "Get server status", body = StatusResponse)
    )
)]
pub async fn get_status() -> impl IntoResponse {
    let status = StatusResponse {
        status: "running".to_string(),
    };

    axum::Json(status)
}

#[utoipa::path(
    get,
    path = "/api/app_working_dir",
    tag = "ruci",
    responses(
        (status = 200, description = "get app working dir", body = String)
    )
)]
pub async fn app_working_dir() -> String {
    let r = current_dir();
    format!("{r:?}")
}

#[cfg(feature = "file_server")]
fn serve_folder_by_tar_data_source_base64(
    mut app: Router,
    file_server_tar_data_source_base64: String,
) -> anyhow::Result<Router> {
    use base64::Engine;
    use data_source::file_server::*;
    use data_source::DataSource;
    let zip_data = {
        match base64::engine::general_purpose::STANDARD.decode(&file_server_tar_data_source_base64)
        {
            Ok(d) => d,
            Err(_) => base64::engine::general_purpose::STANDARD.decode(std::fs::read_to_string(
                &file_server_tar_data_source_base64,
            )?)?,
        }
    };

    use anyhow::Context;
    use std::io::Cursor;
    use zip::ZipArchive;
    let cursor = Cursor::new(zip_data);
    let mut archive = ZipArchive::new(cursor).context("Failed to open ZIP archive")?;
    if archive.len() != 1 {
        bail!("Expected exactly one file in the ZIP archive");
    }

    use std::io::Read;
    let mut tar_file = archive
        .by_index(0)
        .context("Failed to read TAR file in ZIP archive")?;
    let mut tar_data = Vec::new();
    debug!("tarfile is {}", tar_file.name());
    tar_file
        .read_to_end(&mut tar_data)
        .context("Failed to read TAR file contents")?;

    let data_source = DataSource::TarInMemory(tar_data);
    app = register_data_source_route(app, "/files/{*path}", data_source);

    Ok(app)
}

/// non-blocking, it calls tokio::spawn
pub async fn serve(
    s: &Server,
    global_traffic: Arc<ruci::net::GlobalTrafficRecorder>,
    start_core_opts: Opts,
    extension_api_doc: Option<utoipa::openapi::OpenApi>,

    #[cfg(feature = "file_server")] file_server_tar_zip_data_source_base64: Option<String>,
) -> anyhow::Result<()> {
    let addr = s
        .listen_addr
        .clone()
        .unwrap_or_else(|| String::from(crate::DEFAULT_API_ADDR));
    info!("api server starting {addr}");

    let mut app = Router::new().route("/api/status", get(get_status));

    #[cfg(feature = "file_server")]
    {
        match file_server_tar_zip_data_source_base64 {
            None => app = app.nest_service("/dist", tower_http::services::ServeDir::new("dist")),
            Some(file_server_tar_data_source_base64) => {
                app = serve_folder_by_tar_data_source_base64(
                    app,
                    file_server_tar_data_source_base64,
                )?;
            }
        }
    }

    app = app
        .route(
            "/api/engine/stop",
            get(stop_engine).with_state(s.close_engine_tx.clone()),
        )
        .route(
            "/api/engine/start",
            post(start_engine).with_state(start_core_opts),
        )
        .route("/api/app_working_dir", get(app_working_dir))
        .route(
            "/api/traffic/connections/alive/count",
            get(get_alive_conn_count).with_state(global_traffic.clone()),
        )
        .route(
            "/api/traffic/connections/last/id",
            get(get_last_conn_id).with_state(global_traffic.clone()),
        )
        .route(
            "/api/traffic/upload",
            get(get_gt_u).with_state(global_traffic.clone()),
        )
        .route(
            "/api/traffic/download",
            get(get_gt_d).with_state(global_traffic.clone()),
        )
        .route(
            "/api/connections",
            get(get_conn_infos).with_state(s.new_conn_info_map.clone()),
        )
        .route(
            "/api/connections/last/ok",
            get(get_last_ok_cid).with_state(s.new_conn_info_map.clone()),
        )
        .route(
            "/api/connections/range/{cid}",
            get(get_conn_infos_range).with_state(s.new_conn_info_map.clone()),
        )
        .route(
            "/api/connections/count",
            get(get_conn_count).with_state(s.new_conn_info_map.clone()),
        )
        .route(
            "/api/connections/{cid}",
            get(get_conn_info).with_state(s.new_conn_info_map.clone()),
        );

    // 添加扩展API
    for (path, handler) in s.api_extensions.read().iter() {
        app = app.route(path, handler.clone());
        debug!("Added extension API: {}", path);
    }

    #[cfg(feature = "trace")]
    {
        let ism = s.flux_trace.is_monitoring.clone();

        app = app.route(
            "/api/monitoring/status",
            get(is_monitoring_flux).with_state(ism.clone()),
        );
        app = app.route(
            "/api/monitoring/enable",
            get(enable_monitor).with_state(ism.clone()),
        );
        app = app.route(
            "/api/monitoring/disable",
            get(disable_monitor).with_state(ism.clone()),
        );

        app = app.route(
            "/api/traffic/download/{cid}",
            get(get_flux_for).with_state(s.flux_trace.d_cache.clone()),
        );

        app = app.route(
            "/api/traffic/upload/{cid}",
            get(get_flux_for).with_state(s.flux_trace.u_cache.clone()),
        );
    }

    // Add OpenAPI documentation and Swagger UI for core APIs
    app = app.merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()));

    // Add OpenAPI documentation and Swagger UI for extension APIs if provided
    if let Some(extension_doc) = extension_api_doc {
        app = app.merge(
            SwaggerUi::new("/swagger-ui-ext").url("/api-docs-ext/openapi.json", extension_doc),
        );
    }

    // RUST_LOG=tower_http=trace

    use axum::http::Method;
    use tower_http::cors::{Any, CorsLayer};
    use tower_http::trace::TraceLayer;

    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tokio::spawn(async move {
        let r = axum::serve(
            listener,
            app.layer(TraceLayer::new_for_http()).layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods([Method::GET, Method::POST])
                    .allow_headers(Any),
            ),
        )
        .await;

        debug!("api server finished with {r:?}");
    });

    info!("api server started {addr}");

    Ok(())
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

use serde::{Deserialize, Serialize};
use utoipa::OpenApi;

/// Connection information response
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct ConnectionInfoResponse {
    /// Connection ID
    pub cid: String,
    /// Connection timestamp
    pub timestamp: String,
    /// Connection details
    pub details: String,
}

/// Traffic statistics response
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct TrafficStatsResponse {
    /// Number of bytes
    pub bytes: u64,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        get_status,
        stop_engine,
        start_engine,
        get_alive_conn_count,
        get_last_conn_id,
        get_gt_u,
        get_gt_d,
        get_conn_infos,
        get_last_ok_cid,
        get_conn_infos_range,
        get_conn_count,
        get_conn_info,
    ),
    components(
        schemas(StatusResponse, ConnectionInfoResponse, TrafficStatsResponse, crate::modes::CoreArgs, crate::modes::Mode, crate::modes::LevelWrapper)
    ),
    tags(
        (name = "ruci", description = "Ruci API endpoints")
    )
)]
pub struct ApiDoc;

#[cfg(feature = "trace")]
#[utoipa::path(
    get,
    path = "/api/traffic/download/{cid}",
    tag = "ruci",
    params(
        ("cid" = String, Path, description = "Connection ID")
    ),
    responses(
        (status = 200, description = "Get download traffic for a specific connection", body = String)
    )
)]
pub fn get_flux_for_download() {}

#[cfg(feature = "trace")]
#[utoipa::path(
    get,
    path = "/api/traffic/upload/{cid}",
    tag = "ruci",
    params(
        ("cid" = String, Path, description = "Connection ID")
    ),
    responses(
        (status = 200, description = "Get upload traffic for a specific connection", body = String)
    )
)]
pub fn get_flux_for_upload() {}
