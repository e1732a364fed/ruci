/*!
 *
与 verysimple 一样, 我们直接使用 maxmind 的 数据 作为ip国别判断的数据库

https://github.com/Loyalsoldier/geoip

https://github.com/oschwald/maxminddb-rust

https://docs.rs/maxminddb/latest/maxminddb/struct.Reader.html
 */
use std::net::IpAddr;

use anyhow::anyhow;
use log::warn;
use maxminddb::geoip2;

pub const MMDB_DOWNLOAD_LINK: &str =
    "https://cdn.jsdelivr.net/gh/Loyalsoldier/geoip@release/Country.mmdb";

/// try read file  in possible_addrs
pub fn get_ip_iso(ip: IpAddr, filename: &str, possible_addrs: &[&str]) -> String {
    let reader = open_mmdb(filename, possible_addrs).expect(&format!("has {}", filename));

    get_ip_iso_by_reader(ip, reader)
}

pub fn open_mmdb(
    file_name: &str,
    possible_addrs: &[&str],
) -> anyhow::Result<maxminddb::Reader<Vec<u8>>> {
    let mut last_e: Option<maxminddb::MaxMindDBError> = None;
    for dir in possible_addrs {
        let s = String::from(*dir) + file_name;

        let r = maxminddb::Reader::open_readfile(s);
        match r {
            Ok(r) => return Ok(r),
            Err(e) => last_e = Some(e),
        }
    }
    match last_e {
        Some(e) => Err(e.into()),
        None => Err(anyhow!("open_mmdb {file_name} failed and no result err")),
    }
}

/// get iso 3166 string
pub fn get_ip_iso_by_reader(ip: IpAddr, reader: maxminddb::Reader<Vec<u8>>) -> String {
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

#[cfg(test)]
mod test {

    use crate::COMMON_DIRS;

    use super::get_ip_iso;

    #[test]
    fn test1() {
        let s = get_ip_iso("127.0.0.1".parse().unwrap(), "Country.mmdb", &COMMON_DIRS);
        println!("{s}");
        assert_eq!(s, "PRIVATE");

        //www.baidu.com's IP
        let s = get_ip_iso(
            "104.193.88.123".parse().unwrap(),
            "Country.mmdb",
            &COMMON_DIRS,
        );
        println!("{s}");
        assert_eq!(s, "CN");
    }
}
