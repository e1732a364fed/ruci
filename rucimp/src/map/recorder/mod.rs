/*!
Defines a [`Map`] that records the traffic bytes of the base connection.

format: [`RecordData`]， [`SimplifiedRecordData`]

Write to a file record_{}.log when the connection's writer got closed.
*/

pub mod data;
mod tcp;
mod udp;

use std::time;

use addr_conn::AddrConn;
use async_trait::async_trait;
use ruci::map::{self, *};
use ruci::net::*;
use serde::{Deserialize, Serialize};

use macro_map::{map_ext_fields, MapExt};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    pub label: Option<String>,
    pub serialize_format: Option<String>,
    pub full_record: Option<bool>,

    pub no_truncate: Option<bool>,

    pub piece_truncate: Option<usize>,
    pub session_truncate: Option<usize>,
}

impl From<Config> for MapBox {
    fn from(value: Config) -> Self {
        Box::new(RecorderMap::new(value))
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
// impl Name for RecorderMap {
//     fn name(&self) -> &'static str {
//         "recorder"
//     }
// }

#[async_trait]
impl Map for RecorderMap {
    async fn maps(&self, cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        let sgd = params.g.as_ref().map(data::SerializableGlobalData::from);

        let r = data::Recorder {
            start: time::Instant::now(),
            data: data::Record {
                cid: cid.to_string(),
                behavior,
                // global_data: params.g.clone(),
                global_data: sgd.clone(),
                full_record: self.config.full_record,
                piece_truncate: self.config.piece_truncate,
                session_truncate: self.config.session_truncate,
                no_truncate: self.config.no_truncate,

                serialize_format: self.config.serialize_format.clone(),
                label: self.config.label.clone(),

                ..Default::default()
            },
        };

        match params.c {
            Stream::Conn(c) => {
                let rc = tcp::RecorderConn {
                    base: Box::pin(c),
                    record: r,
                };

                MapResult::builder()
                    .a(params.a)
                    .b(params.b)
                    .c(Stream::c(Box::new(rc)))
                    .build()
            }
            Stream::AddrConn(ac) => {
                // 由于 AddrConn的实现是拆成 r,w 两部分的， 为避免多线程冲突，这里简单地拆成两个文件

                let mut r_r = r;
                let mut w_r = r_r.clone();
                r_r.data.file_prefix = Some(String::from("ac_r"));
                w_r.data.file_prefix = Some(String::from("ac_w"));

                let ac = AddrConn {
                    r: Box::new(udp::RecordAddrConnR {
                        base: Box::pin(ac.r),
                        record: r_r,
                    }),
                    w: Box::new(udp::RecordAddrConnW {
                        base: Box::pin(ac.w),
                        record: w_r,
                    }),
                    default_write_to: ac.default_write_to,
                    // cached_name: "record_ac".to_string(),
                };

                MapResult::builder()
                    .a(params.a)
                    .b(params.b)
                    .c(Stream::AddrConn(ac))
                    .build()
            }
            Stream::None => MapResult::from_err_str("recorder: can't init without a stream"),
            _ => MapResult::from_err_str(&format!(
                "recorder: can't init with type of {:?}",
                params.c
            )),
        }
    }
}

#[cfg(test)]
mod test {

    use std::time;

    use ruci::map::GlobalData;

    use crate::map::recorder::data::SerializableGlobalData;

    #[test]
    fn time() {
        let gd = GlobalData {
            instance_start_time: Some(time::SystemTime::now()),
            ..Default::default()
        };
        let sgd = SerializableGlobalData::from(&gd);
        println!("sgd {:?}  ", sgd.instance_start_time);

        let gd2: GlobalData = (&sgd).into();
        println!("gd2   {:?}", gd2.instance_start_time);

        let sgd2: SerializableGlobalData = SerializableGlobalData::from(&gd2);
        println!("sgd2   {:?}", sgd2.instance_start_time);

        assert_eq!(sgd2.instance_start_time, sgd.instance_start_time)
    }
}
