mod tcp;

use std::fmt::Display;
use std::path::PathBuf;
use std::{pin::Pin, time};

use async_trait::async_trait;
use chrono::DateTime;
use futures::Future;
use macro_map::{map_ext_fields, MapExt};
use ruci::map::{self, Map, MapBox, MapParams, MapResult};
use ruci::net::{Stream, CID};
use ruci::{
    map::{GlobalData, ProxyBehavior},
    net::Addr,
};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tracing::debug;

pub const UPLOAD_DIRECTION: i8 = 1;
pub const DOWNLOAD_DIRECTION: i8 = -1;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum OutputFileExtension {
    #[default]
    Json,
    Cbor,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum OutputFormat {
    #[default]
    Ruci,
    Har,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum RecordMode {
    #[default]
    Full,
    Simplified,
    Info,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum PieceTruncateOption {
    #[default]
    NoTruncate,
    PieceTruncate(usize),
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum SessionTruncateOption {
    #[default]
    NoTruncate,
    SessionTruncate(usize),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub label: Option<String>,
    pub output_dir: String,
    pub output_file_extension: OutputFileExtension,
    pub output_format: OutputFormat,
    pub record_mode: RecordMode,

    pub piece_truncate_option: Option<PieceTruncateOption>,
    pub session_truncate_option: Option<SessionTruncateOption>,
}

pub enum Recorder {
    Full(FullRecorder),
    Simplified(SimplifiedRecorder),
    Info(InfoRecorder),
}

impl Recorder {
    pub fn cid(&self) -> &str {
        match self {
            Recorder::Full(r) => r.cid(),
            Recorder::Simplified(r) => r.cid(),
            Recorder::Info(r) => r.cid(),
        }
    }

    pub fn since(&self) -> u128 {
        match self {
            Recorder::Full(r) => r.since(r.start),
            Recorder::Simplified(r) => r.since(r.start),
            Recorder::Info(r) => r.since(r.start),
        }
    }

    pub fn record_u(&mut self, data: &[u8]) {
        match self {
            Recorder::Full(r) => r.record_u(data),
            Recorder::Simplified(r) => r.record_u(data),
            Recorder::Info(r) => r.record_u(data),
        }
    }

    pub fn record_d(&mut self, data: &[u8]) {
        match self {
            Recorder::Full(r) => r.record_d(data),
            Recorder::Simplified(r) => r.record_d(data),
            Recorder::Info(r) => r.record_d(data),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Recorder::Full(r) => r.data.label.as_deref().unwrap_or(""),
            Recorder::Simplified(r) => r.data.label.as_deref().unwrap_or(""),
            Recorder::Info(r) => r.data.label.as_deref().unwrap_or(""),
        }
    }

    pub fn async_save(&self) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + Sync>> {
        match self {
            Recorder::Full(r) => {
                let config = r.config.clone();
                let r = r.clone();
                Box::pin(async move { r.async_save_to_file(&config).await })
            }
            Recorder::Simplified(r) => {
                let config = r.config.clone();
                let r = r.clone();
                Box::pin(async move { r.async_save_to_file(&config).await })
            }
            Recorder::Info(r) => {
                let config = r.config.clone();
                let r = r.clone();
                Box::pin(async move { r.async_save_to_file(&config).await })
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct FullRecorder {
    pub config: Config,

    pub data: FullData,

    pub start: time::Instant,
}

#[derive(Debug, Clone)]

pub struct SimplifiedRecorder {
    pub config: Config,

    pub data: SimplifiedRecordData,

    pub start: time::Instant,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct FullData {
    pub cid: String,
    pub behavior: ProxyBehavior,

    /// customized by user (as a marker)
    pub label: Option<String>,

    pub global_data: Option<SerializableGlobalData>,

    pub upload_data: Vec<FullPayloadData>,
    pub download_data: Vec<FullPayloadData>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum FullPayloadData {
    /// 0 为 时间戳， 1 为 数据
    Tcp(u128, Vec<u8>),
    /// 0 为 时间戳， 1 为 地址， 2 为 数据
    Udp((u128, Addr, Vec<u8>)),
}

/// SimplifiedRecordData 没有时序信息, 也不包含地址信息
///
/// mainly for export data for machine learning
///
///
#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct SimplifiedRecordData {
    pub cid: String,

    pub label: Option<String>,
    pub data: Vec<(i8, Vec<u8>)>, // 使用 UPLOAD_DIRECTION 和 DOWNLOAD_DIRECTION
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct SerializableGlobalData {
    pub run_instance_id: u32,

    pub instance_start_time: Option<chrono::DateTime<chrono::Utc>>,

    pub read_handshake_timeout: Option<u64>,
}

impl From<&GlobalData> for SerializableGlobalData {
    fn from(gd: &GlobalData) -> Self {
        SerializableGlobalData {
            run_instance_id: gd.run_instance_id,
            read_handshake_timeout: gd.read_handshake_timeout,
            instance_start_time: gd.instance_start_time.map(|st| {
                // chrono-0.4.38/src/offset/utc.rs
                let duration = st
                    .duration_since(time::UNIX_EPOCH)
                    .expect("system time before Unix epoch");
                chrono::DateTime::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos())
                    .unwrap()
            }),
        }
    }
}

impl From<&SerializableGlobalData> for GlobalData {
    fn from(val: &SerializableGlobalData) -> Self {
        GlobalData {
            run_instance_id: val.run_instance_id,
            read_handshake_timeout: val.read_handshake_timeout,
            instance_start_time: val.instance_start_time.map(|dt| {
                let epoch = DateTime::from_timestamp(0, 0).unwrap();
                let delta = dt.signed_duration_since(epoch);
                let secs = delta.num_seconds() as u64;
                let nanos = delta.subsec_nanos() as u32;
                time::UNIX_EPOCH + time::Duration::new(secs, nanos)
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct InfoRecorder {
    pub config: Config,

    pub data: InfoData,

    pub start: time::Instant,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct InfoData {
    pub cid: String,
    pub behavior: ProxyBehavior,
    pub label: Option<String>,

    pub global_data: Option<SerializableGlobalData>,

    pub payload: Vec<PayloadInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PayloadInfo {
    /// 0 为 时间戳， 1 为 数据(1:upload, -1:download), 2 为 数据长度
    Tcp(u128, i8, usize),
    /// 0 为 时间戳， 1 为 地址， 2 为 数据方向(1:upload, -1:download), 3 为 数据长度
    Udp(u128, Addr, i8, usize),
}

impl Default for PayloadInfo {
    fn default() -> Self {
        PayloadInfo::Tcp(0, 0, 0)
    }
}

impl InfoRecorder {
    pub fn record_u(&mut self, data: &[u8]) {
        let d = self.since(self.start);
        self.data
            .payload
            .push(PayloadInfo::Tcp(d, UPLOAD_DIRECTION, data.len()));
    }

    pub fn record_d(&mut self, data: &[u8]) {
        let d = self.since(self.start);
        self.data
            .payload
            .push(PayloadInfo::Tcp(d, DOWNLOAD_DIRECTION, data.len()));
    }
}

impl SimplifiedRecorder {
    pub fn record_u(&mut self, data: &[u8]) {
        self.data.data.push((UPLOAD_DIRECTION, data.to_vec()));
    }

    pub fn record_d(&mut self, data: &[u8]) {
        self.data.data.push((DOWNLOAD_DIRECTION, data.to_vec()));
    }
}

impl FullRecorder {
    pub fn record_u(&mut self, data: &[u8]) {
        let d = self.since(self.start);
        self.data
            .upload_data
            .push(FullPayloadData::Tcp(d, data.to_vec()));
    }

    pub fn record_d(&mut self, data: &[u8]) {
        let d = self.since(self.start);
        self.data
            .download_data
            .push(FullPayloadData::Tcp(d, data.to_vec()));
    }
}

// Common trait for all recorders
pub trait RecorderTrait {
    fn cid(&self) -> &str;
    fn since(&self, start: time::Instant) -> u128 {
        start.elapsed().as_nanos()
    }
    // fn save_to_file(&self, config: &Config) -> std::io::Result<()>;
    fn async_save_to_file(
        self,
        config: &Config,
    ) -> impl Future<Output = std::io::Result<()>> + Send;
}

// Common error type for serialization
#[derive(Debug)]
enum SerializeError {
    Json(serde_json::Error),
    Cbor(serde_cbor::Error),
}

impl std::fmt::Display for SerializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SerializeError::Json(e) => write!(f, "JSON error: {}", e),
            SerializeError::Cbor(e) => write!(f, "CBOR error: {}", e),
        }
    }
}

impl std::error::Error for SerializeError {}

// Common save implementation for async
async fn async_save_to_file<T: serde::Serialize + Data + Send + 'static + Clone>(
    data: T,
    cid: &str,
    config: &Config,
) -> std::io::Result<()> {
    // Create output directory if not exists
    tokio::fs::create_dir_all(&config.output_dir).await?;

    let mut file_path = PathBuf::from(&config.output_dir);

    let format = format!("{:?}", data.format()).to_lowercase();

    let mode = format!("{:?}", config.record_mode).to_lowercase();

    file_path.push(match config.output_file_extension {
        OutputFileExtension::Json => {
            format!("{}_{}_{}_{}.json", cid, data.get_label(), format, mode)
        }
        OutputFileExtension::Cbor => {
            format!("{}_{}_{}_{}.cbor", cid, data.get_label(), format, mode)
        }
    });

    // Clone the data for the blocking task
    let ext = config.output_file_extension;

    // Spawn blocking task for serialization since serde operations are CPU-bound (especially for large data)
    let buf = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, SerializeError> {
        match ext {
            OutputFileExtension::Json => serde_json::to_vec(&data).map_err(SerializeError::Json),
            OutputFileExtension::Cbor => serde_cbor::to_vec(&data).map_err(SerializeError::Cbor),
        }
    })
    .await
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    if buf.is_empty() {
        debug!("serde got empty data: {}", file_path.display());
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "empty data".to_string(),
        ));
    } else {
        debug!(
            "serde got data: {}, file: {}",
            buf.len(),
            file_path.display()
        );
    }

    let mut file = tokio::fs::File::create(&file_path).await?;

    debug!("file created: {}", file_path.display());

    file.write_all(&buf).await?;
    file.sync_all().await?;

    file.flush().await?;
    file.sync_all().await?;

    debug!("saved to file: {}", file_path.display());

    Ok(())
}

impl RecorderTrait for InfoRecorder {
    fn cid(&self) -> &str {
        &self.data.cid
    }

    async fn async_save_to_file(self, config: &Config) -> std::io::Result<()> {
        let cid = self.cid().to_string();
        match config.output_format {
            OutputFormat::Ruci => async_save_to_file(self.data, &cid, config).await,
            OutputFormat::Har => {
                let har: har::Har = self.data.into();
                async_save_to_file(
                    har,
                    &cid,
                    &Config {
                        output_file_extension: config.output_file_extension,
                        ..Default::default()
                    },
                )
                .await
            }
        }
    }
}

impl RecorderTrait for SimplifiedRecorder {
    fn cid(&self) -> &str {
        &self.data.cid
    }

    async fn async_save_to_file(self, config: &Config) -> std::io::Result<()> {
        let cid = self.cid().to_string();
        match config.output_format {
            OutputFormat::Ruci => async_save_to_file(self.data, &cid, config).await,
            OutputFormat::Har => {
                let har: har::Har = self.data.into();
                async_save_to_file(
                    har,
                    &cid,
                    &Config {
                        output_file_extension: config.output_file_extension,
                        ..Default::default()
                    },
                )
                .await
            }
        }
    }
}

impl RecorderTrait for FullRecorder {
    fn cid(&self) -> &str {
        &self.data.cid
    }

    async fn async_save_to_file(self, config: &Config) -> std::io::Result<()> {
        let cid = self.cid().to_string();
        match config.output_format {
            OutputFormat::Ruci => async_save_to_file(self.data, &cid, config).await,
            OutputFormat::Har => panic!("har is not implemented for full data"),
        }
    }
}

#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct RecorderMap {
    config: Config,
}

impl RecorderMap {
    pub fn new(config: Config) -> RecorderMap {
        RecorderMap {
            config,
            ..Default::default()
        }
    }
}

impl Display for RecorderMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "recorder")
    }
}

impl From<Config> for MapBox {
    fn from(value: Config) -> Self {
        Box::new(RecorderMap::new(value))
    }
}

#[async_trait]
impl Map for RecorderMap {
    async fn maps(&self, cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let r = match self.config.record_mode {
            RecordMode::Full => {
                let mut r = FullRecorder {
                    config: self.config.clone(),
                    data: FullData::default(),
                    start: time::Instant::now(),
                };

                r.data.cid = cid.to_string();
                r.data.behavior = behavior;
                r.data.label = self.config.label.clone();
                r.data.global_data = params.g.as_ref().map(SerializableGlobalData::from);

                Recorder::Full(r)
            }
            RecordMode::Simplified => {
                let mut r = SimplifiedRecorder {
                    config: self.config.clone(),
                    data: SimplifiedRecordData::default(),
                    start: time::Instant::now(),
                };

                r.data.cid = cid.to_string();
                r.data.label = self.config.label.clone();

                Recorder::Simplified(r)
            }
            RecordMode::Info => {
                let mut r = InfoRecorder {
                    config: self.config.clone(),
                    data: InfoData::default(),
                    start: time::Instant::now(),
                };

                r.data.cid = cid.to_string();
                r.data.behavior = behavior;
                r.data.label = self.config.label.clone();
                r.data.global_data = params.g.as_ref().map(SerializableGlobalData::from);

                Recorder::Info(r)
            }
        };

        match params.c {
            Stream::Conn(c) => {
                let rc = tcp::RecorderConn {
                    base: Box::pin(c),
                    record: r,
                    save_future: None,
                    state: tcp::State::Normal,
                };
                MapResult::builder()
                    .a(params.a)
                    .b(params.b)
                    .c(Stream::c(Box::new(rc)))
                    .build()
            }
            Stream::AddrConn(_addr_conn) => todo!(),
            _ => todo!(),
        }
    }
}

pub trait Data {
    fn get_label(&self) -> &str {
        self.label().as_deref().unwrap_or("")
    }

    fn label(&self) -> &Option<String>;
    fn format(&self) -> OutputFormat;
}

impl Data for InfoData {
    fn label(&self) -> &Option<String> {
        &self.label
    }

    fn format(&self) -> OutputFormat {
        OutputFormat::Ruci
    }
}

impl Data for SimplifiedRecordData {
    fn label(&self) -> &Option<String> {
        &self.label
    }

    fn format(&self) -> OutputFormat {
        OutputFormat::Ruci
    }
}

impl Data for FullData {
    fn label(&self) -> &Option<String> {
        &self.label
    }

    fn format(&self) -> OutputFormat {
        OutputFormat::Ruci
    }
}

impl Data for har::Har {
    fn label(&self) -> &Option<String> {
        match &self.log {
            har::Spec::V1_2(v) => &v.creator.comment,
            har::Spec::V1_3(v) => &v.creator.comment,
        }
    }

    fn format(&self) -> OutputFormat {
        OutputFormat::Har
    }
}
impl From<PayloadInfo> for har::v1_2::Entries {
    fn from(val: PayloadInfo) -> Self {
        match val {
            PayloadInfo::Tcp(t, d, l) => {
                let mut e = har::v1_2::Entries {
                    time: t as f64,
                    ..Default::default()
                };

                if d == 1 {
                    e.request = har::v1_2::Request {
                        body_size: l as i64,
                        ..Default::default()
                    };
                } else {
                    e.response = har::v1_2::Response {
                        body_size: l as i64,
                        ..Default::default()
                    };
                }

                e
            }
            PayloadInfo::Udp(t, a, d, l) => {
                let mut e = har::v1_2::Entries {
                    time: t as f64,
                    ..Default::default()
                };

                if d == 1 {
                    e.request = har::v1_2::Request {
                        body_size: l as i64,
                        url: a.to_string(),
                        ..Default::default()
                    };
                } else {
                    e.response = har::v1_2::Response {
                        body_size: l as i64,
                        redirect_url: Some(a.to_string()),
                        ..Default::default()
                    };
                }

                e
            }
        }
    }
}

impl From<InfoData> for har::Har {
    fn from(val: InfoData) -> Self {
        let entries = val
            .payload
            .into_iter()
            .map(|p| p.into())
            .collect::<Vec<_>>();

        har::Har {
            log: har::Spec::V1_2(har::v1_2::Log {
                creator: har::v1_2::Creator {
                    name: "ruci".to_string(),
                    version: crate::VERSION.to_string(),
                    comment: None,
                },

                entries,
                ..Default::default()
            }),
        }
    }
}

impl From<SimplifiedRecordData> for har::Har {
    fn from(val: SimplifiedRecordData) -> Self {
        let entries = val
            .data
            .into_iter()
            .map(|(direction, data)| PayloadInfo::Tcp(0, direction, data.len()))
            .collect::<Vec<_>>();

        let entries = entries.into_iter().map(|p| p.into()).collect::<Vec<_>>();

        har::Har {
            log: har::Spec::V1_2(har::v1_2::Log {
                entries,
                ..Default::default()
            }),
        }
    }
}
