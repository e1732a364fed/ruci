/*!
 *
与 verysimple 一样, 我们直接使用 maxmind 的 数据 作为ip国别判断的数据库

https://github.com/Loyalsoldier/geoip

https://github.com/oschwald/maxminddb-rust

https://docs.rs/maxminddb/latest/maxminddb/struct.Reader.html
 */
use std::net::IpAddr;

use maxminddb::geoip2;

pub fn get_ip_iso(ip: IpAddr) -> String {
    let reader = maxminddb::Reader::open_readfile("resource/Country.mmdb").unwrap();

    let c: geoip2::Country = reader.lookup(ip).unwrap();
    c.country.unwrap().iso_code.unwrap().to_string()
}
//todo: use real file; cache in mem; remove unwrap; add test
