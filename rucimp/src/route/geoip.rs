/*!
 *
与 verysimple 一样, 我们直接使用 maxmind 的 数据 作为ip国别判断的数据库

https://github.com/Loyalsoldier/geoip

https://github.com/oschwald/maxminddb-rust

https://docs.rs/maxminddb/latest/maxminddb/struct.Reader.html
 */
use std::net::IpAddr;

use log::warn;
use maxminddb::geoip2;

pub fn get_ip_iso(ip: IpAddr) -> String {
    let reader = maxminddb::Reader::open_readfile("resource/Country.mmdb")
        .expect("has resource/Country.mmdb");

    let r = reader.lookup(ip);
    let c: geoip2::Country = match r {
        Ok(c) => c,
        Err(e) => {
            warn!("look up maxminddb::Reader failed, {e}");
            return "".to_string();
        }
    };
    if let Some(c) = c.country {
        c.iso_code.unwrap_or_default().to_string()
    } else {
        "".to_string()
    }
}
//todo: use real file; cache in mem;  add test
