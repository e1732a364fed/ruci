/*!
Defines a [`Map`] that records the traffic bytes of the base connection.

format: [`RecordData`]， [`RecordData`]

Write to a file record_{}.log when the connection's writer got closed.
*/

use super::*;
use std::task::Context;
use std::time;
use std::{io, pin::Pin, task::Poll};

use crate::map;
use crate::{net::*, Name};
use addr_conn::{AsyncReadAddr, AsyncWriteAddr};
use async_trait::async_trait;
use chrono::DateTime;
use tokio::io::{AsyncRead, AsyncWrite};

use macro_map::{map_ext_fields, MapExt};
use tracing::info;

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
                let x = st
                    .duration_since(time::UNIX_EPOCH)
                    .expect("system time before Unix epoch");
                chrono::DateTime::from_timestamp(x.as_secs() as i64, x.subsec_nanos()).unwrap()
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

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct RecordData {
    pub cid: String,
    pub behavior: ProxyBehavior,

    #[serde(skip)]
    pub global_data: Option<GlobalData>,

    #[serde(skip)]
    pub file_prefix: Option<String>,

    pub serializable_global_data: Option<SerializableGlobalData>,

    /// customized by user (as a marker)
    pub custom_str: String,
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

impl RecordData {
    fn save(&self) {
        let mut name = if let Some(g) = &self.serializable_global_data {
            format!("record_{}_{}.log", g.run_instance_id, self.cid)
        } else {
            format!("record_{}.log", self.cid)
        };
        if let Some(p) = &self.file_prefix {
            name = p.to_owned() + &name;
        }

        let r = serde_json::to_writer_pretty(std::fs::File::create(name).unwrap(), &self);
        if let Err(e) = r {
            tracing::warn!("save to file got error: {e}");
        }
    }
}

#[derive(Clone)]
struct Recorder {
    start: time::Instant,
    data: RecordData,
}
impl Recorder {
    fn since(&self) -> u128 {
        time::Instant::now().duration_since(self.start).as_nanos()
    }

    fn record_d(&mut self, data: &[u8]) {
        let d = self.since();
        let d = DataPiece {
            nanos_since_start: d,
            data: PayloadData::Pure(data.to_vec()),
        };
        self.data.download_data.push(d)
    }
    fn r_d_ad(&mut self, data: &[u8], ad: &Addr) {
        let d = self.since();
        self.data.download_data.push(DataPiece {
            nanos_since_start: d,
            data: PayloadData::Addr((ad.clone(), data.to_vec())),
        });
    }
    fn record_u(&mut self, data: &[u8]) {
        let d = self.since();
        self.data.upload_data.push(DataPiece {
            nanos_since_start: d,
            data: PayloadData::Pure(data.to_vec()),
        })
    }

    fn r_u_ad(&mut self, data: &[u8], ad: &Addr) {
        let d = self.since();
        self.data.upload_data.push(DataPiece {
            nanos_since_start: d,
            data: PayloadData::Addr((ad.clone(), data.to_vec())),
        })
    }
}
/// takes ownership of base Conn
struct RecorderConn {
    base: Pin<net::Conn>,
    record: Recorder,
}

impl Name for RecorderConn {
    fn name(&self) -> &str {
        "recorder"
    }
}

impl AsyncRead for RecorderConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let r = self.base.as_mut().poll_read(cx, buf);
        if let Poll::Ready(r) = &r {
            match r {
                Ok(_) => self.record.record_d(buf.filled()),
                Err(e) => {
                    info!(
                        cid = %self.record.data.cid,
                        "recorder read got err, Saving to file; err: {e}",
                    );

                    self.record.data.save();
                }
            }
        }
        r
    }
}

impl AsyncWrite for RecorderConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let r = self.base.as_mut().poll_write(cx, buf);

        if let Poll::Ready(Ok(u)) = &r {
            self.record.record_u(&buf[..*u]);
        }
        r
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        self.base.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        info!(
            cid = %self.record.data.cid,
            "recorder got shutdown, Saving to file...",
        );

        self.record.data.save();
        self.base.as_mut().poll_shutdown(cx)
    }
}

struct RecordAddrConnR {
    base: Pin<Box<dyn addr_conn::AddrReadTrait>>,
    record: Recorder,
}

impl crate::Name for RecordAddrConnR {
    fn name(&self) -> &str {
        "recorder_ac_r"
    }
}

struct RecordAddrConnW {
    base: Pin<Box<dyn addr_conn::AddrWriteTrait>>,
    record: Recorder,
}

impl crate::Name for RecordAddrConnW {
    fn name(&self) -> &str {
        "recorder_ac_w"
    }
}

impl AsyncReadAddr for RecordAddrConnR {
    fn poll_read_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, Addr)>> {
        let r = self.base.as_mut().poll_read_addr(cx, buf);
        if let Poll::Ready(Ok((n, ad))) = r {
            self.record.r_d_ad(buf, &ad);

            Poll::Ready(io::Result::Ok((n, ad)))
        } else {
            r
        }
    }
}

impl AsyncWriteAddr for RecordAddrConnW {
    fn poll_write_addr(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: &Addr,
    ) -> Poll<io::Result<usize>> {
        let r = self.base.as_mut().poll_write_addr(cx, buf, addr);

        if let Poll::Ready(Ok(n)) = r {
            self.record.r_u_ad(&buf[..n], addr);

            Poll::Ready(io::Result::Ok(n))
        } else {
            r
        }
    }

    fn poll_flush_addr(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.base.as_mut().poll_flush_addr(cx)
    }

    fn poll_close_addr(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        info!(
            cid = %self.record.data.cid,
            "recorder ac got shutdown, Saving to file..."
        );
        self.record.data.save();

        self.base.as_mut().poll_close_addr(cx)
    }
}

#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct RecorderMap {
    pub custom_str: String,
}

impl Name for RecorderMap {
    fn name(&self) -> &'static str {
        "recorder"
    }
}

#[async_trait]
impl Map for RecorderMap {
    async fn maps(&self, cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let now = time::Instant::now();
        let sgd = params.g.as_ref().map(SerializableGlobalData::from);

        let rb = Recorder {
            start: now,
            data: RecordData {
                cid: cid.to_string(),
                behavior,
                global_data: params.g.clone(),

                serializable_global_data: sgd.clone(),

                custom_str: self.custom_str.clone(),

                ..Default::default()
            },
        };

        match params.c {
            Stream::Conn(c) => {
                let cc = RecorderConn {
                    base: Box::pin(c),
                    record: rb,
                };

                MapResult::builder()
                    .a(params.a)
                    .b(params.b)
                    .c(Stream::c(Box::new(cc)))
                    .build()
            }
            Stream::AddrConn(ac) => {
                // 由于 AddrConn的实现是拆成 r,w 两部分的， 为避免多线程冲突，这里简单地拆成两个文件

                let mut r_rb = rb;
                let mut w_rb = r_rb.clone();
                r_rb.data.file_prefix = Some(String::from("ac_r"));
                w_rb.data.file_prefix = Some(String::from("ac_w"));

                let ac = AddrConn {
                    r: Box::new(RecordAddrConnR {
                        base: Box::pin(ac.r),
                        record: r_rb,
                    }),
                    w: Box::new(RecordAddrConnW {
                        base: Box::pin(ac.w),
                        record: w_rb,
                    }),
                    default_write_to: ac.default_write_to,
                    cached_name: "record_ac".to_string(),
                };

                MapResult::builder()
                    .a(params.a)
                    .b(params.b)
                    .c(Stream::AddrConn(ac))
                    .build()
            }
            Stream::None => MapResult::err_str("recorder: can't init without a stream"),
            _ => MapResult::err_str("recorder: can't init with a stream generator"),
        }
    }
}

#[cfg(test)]
mod test {

    use std::time;

    use crate::map::{recorder::SerializableGlobalData, GlobalData};

    #[test]
    fn time() {
        let gd = GlobalData {
            instance_start_time: Some(time::SystemTime::now()),
            ..Default::default()
        };
        let sgd = SerializableGlobalData::from(&gd);
        println!("sgd {:?}  ", sgd.instance_start_time);

        let gd2: GlobalData;
        gd2 = (&sgd).into();
        println!("gd2   {:?}", gd2.instance_start_time);

        let sgd2: SerializableGlobalData;
        sgd2 = SerializableGlobalData::from(&gd2);
        println!("sgd2   {:?}", sgd2.instance_start_time);

        assert_eq!(sgd2.instance_start_time, sgd.instance_start_time)
    }
}
