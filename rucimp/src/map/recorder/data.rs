use std::time;

use chrono::DateTime;
use itertools::Itertools;
use ruci::map::*;
use ruci::net::*;
use serde::{Deserialize, Serialize};

/// 调用 Recorder 的方法 将 数据写入其中
#[derive(Clone)]
pub struct Recorder {
    pub start: time::Instant,
    pub data: Record,
}
impl Recorder {
    pub fn since(&self) -> u128 {
        time::Instant::now().duration_since(self.start).as_nanos()
    }

    pub fn record_d(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let d = self.since();
        let d = DataPiece {
            nanos_since_start: d,
            data: PayloadData::Pure(data.to_vec()),
        };
        self.data.download_data.push(d)
    }
    pub fn r_d_ad(&mut self, data: &[u8], ad: &Addr) {
        if data.is_empty() {
            return;
        }
        let d = self.since();
        self.data.download_data.push(DataPiece {
            nanos_since_start: d,
            data: PayloadData::Addr((ad.clone(), data.to_vec())),
        });
    }
    pub fn record_u(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let d = self.since();
        self.data.upload_data.push(DataPiece {
            nanos_since_start: d,
            data: PayloadData::Pure(data.to_vec()),
        })
    }

    pub fn r_u_ad(&mut self, data: &[u8], ad: &Addr) {
        if data.is_empty() {
            return;
        }
        let d = self.since();
        self.data.upload_data.push(DataPiece {
            nanos_since_start: d,
            data: PayloadData::Addr((ad.clone(), data.to_vec())),
        })
    }
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

/// for export data for machine learning
#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct SimplifiedRecordData {
    pub label: Option<String>,
    pub data: Vec<(i8, Vec<u8>)>, // 1:upload, -1: download
}

impl From<&mut Record> for SimplifiedRecordData {
    /// 本转换会 拿走 RecordData 中 download_data 和 upload_data
    ///
    /// 且将截断每段条目的1500字节以上的部分(但不做padding 以最小化文件大小)
    ///
    /// 且总数据量不超过3000
    fn from(d: &mut Record) -> Self {
        let piece_truncate = d.piece_truncate.unwrap_or(1500);
        let session_truncate = d.session_truncate.unwrap_or(3000);
        let truncate = !d.no_truncate.unwrap_or(false);

        let convert = |d: Vec<DataPiece>, i: i8| -> Vec<(i8, u128, Vec<u8>)> {
            d.into_iter()
                .map(|dp| {
                    let mut data = match dp.data {
                        PayloadData::Pure(d) => d,
                        PayloadData::Addr(add) => add.1,
                    };
                    if truncate {
                        data.truncate(piece_truncate);
                    }
                    (i, dp.nanos_since_start, data)
                })
                .collect()
        };

        let mut dd = Vec::new();
        std::mem::swap(&mut d.download_data, &mut dd);

        let mut ud = Vec::new();
        std::mem::swap(&mut d.upload_data, &mut ud);

        let mut a = convert(dd, -1);
        let mut ua = convert(ud, 1);

        a.append(&mut ua);

        a.sort_by(|a, b| a.1.cmp(&b.1));

        let mut s = 0;
        let mut last = 0;
        for (i, x) in a.iter().enumerate() {
            s += x.2.len();
            last = i;
            if s > session_truncate {
                break;
            }
        }

        if truncate {
            a.truncate(last);
        }

        let data = a.into_iter().map(|x| (x.0, x.2)).collect_vec();

        SimplifiedRecordData {
            data,
            label: d.label.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct Record {
    pub cid: String,
    pub behavior: ProxyBehavior,

    /// customized by user (as a marker)
    pub label: Option<String>,

    #[serde(skip)]
    pub file_prefix: Option<String>,

    #[serde(skip)]
    pub serialize_format: Option<String>,

    /// 如果 full_record 没启用，序列化时将直接使用 SimplifiedRecordData 且使用
    /// piece_truncate 和 session_truncate
    #[serde(skip)]
    pub full_record: Option<bool>,

    pub no_truncate: Option<bool>,

    pub piece_truncate: Option<usize>,

    pub session_truncate: Option<usize>,

    pub global_data: Option<SerializableGlobalData>,

    pub upload_data: Vec<DataPiece>,
    pub download_data: Vec<DataPiece>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct DataPiece {
    /// nano seconds since the start of the connection,
    pub nanos_since_start: u128,

    /// data send/recv at this instant
    pub data: PayloadData,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PayloadData {
    Pure(Vec<u8>),
    Addr((Addr, Vec<u8>)),
}
impl Default for PayloadData {
    fn default() -> Self {
        PayloadData::Pure(Vec::new())
    }
}

impl Record {
    /// log/name.log
    pub fn save_name(&self) -> String {
        let tail = format!(
            "{}_{}.{}.log",
            self.label.as_deref().unwrap_or_default(),
            self.cid,
            self.serialize_format.as_deref().unwrap_or("json")
        );
        let name = if let Some(g) = &self.global_data {
            format!("record_{}_{}", g.run_instance_id, tail)
        } else {
            format!("record_.{}", tail)
        };

        format!("logs/{}", name)
    }
    pub fn save(&mut self) {
        let _ = std::fs::create_dir("logs");

        let name = self.save_name();

        let r = match &self.serialize_format {
            Some(s) => match s.as_str() {
                "json" => self.save_json(name),
                "cbor" => self.save_cbor(name),
                _ => self.save_json(name),
            },
            None => self.save_json(name),
        };

        if let Err(e) = r {
            tracing::warn!("save to file got error: {e}");
        }
    }

    pub fn save_cbor(&mut self, name: String) -> anyhow::Result<()> {
        if self.full_record.unwrap_or_default() {
            let f = std::fs::File::create(name).unwrap();

            Ok(serde_cbor::to_writer(f, &self)?)
        } else {
            let sd = SimplifiedRecordData::from(self);
            if !sd.data.is_empty() {
                let f = std::fs::File::create(name).unwrap();

                serde_cbor::to_writer(f, &sd)?
            }
            Ok(())
        }
    }

    pub fn save_json(&mut self, name: String) -> anyhow::Result<()> {
        if self.full_record.unwrap_or_default() {
            let f = std::fs::File::create(name).unwrap();

            Ok(serde_json::to_writer_pretty(f, &self)?)
        } else {
            let sd = SimplifiedRecordData::from(self);
            if !sd.data.is_empty() {
                let f = std::fs::File::create(name).unwrap();

                serde_json::to_writer_pretty(f, &sd)?
            }
            Ok(())
        }
    }
}
