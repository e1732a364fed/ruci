/*!
 *
与 verysimple 一样, 我们直接使用 maxmind 的 数据 作为ip国别判断的数据库

https://www.maxmind.com/en/geoip-databases

https://dev.maxmind.com/geoip/geolite2-free-geolocation-data

https://www.maxmind.com/en/accounts/current/geoip/downloads

https://github.com/oschwald/maxminddb-rust

https://docs.rs/maxminddb/latest/maxminddb/struct.Reader.html

https://github.com/Loyalsoldier/geoip

 */
use std::net::IpAddr;

use anyhow::anyhow;
use maxminddb::geoip2;
use tracing::{debug, warn};

/// try read file  in possible_addrs
pub fn get_ip_iso(ip: IpAddr, filename: &str, possible_addrs: &[&str]) -> String {
    let reader = open_mmdb(filename, possible_addrs).unwrap_or_else(|_| panic!("has {}", filename));

    get_ip_iso_by_reader(ip, &reader)
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
///
/// https://en.wikipedia.org/wiki/List_of_ISO_3166_country_codes
pub fn get_ip_iso_by_reader(ip: IpAddr, reader: &maxminddb::Reader<Vec<u8>>) -> String {
    let r = reader.lookup(ip);
    let c: geoip2::Country = match r {
        Ok(c) => c,
        Err(e) => {
            warn!("look up maxminddb::Reader failed, {e}");
            return "".to_string();
        }
    };
    debug!("got {:?}", c);

    if let Some(c) = c.country {
        c.iso_code.unwrap_or_default().to_string()
    } else {
        "".to_string()
    }
}

/// see  doc/notes.md
///
/// Convert GOOGLE, TWITTER, TELEGRAM, FACEBOOK, NETFLIX, CLOUDFRONT,CLOUDFLARE etc. to US
///
pub fn filter_iso_string_to_iso3166(s: &str) -> &str {
    if s == "PRIVATE" {
        return s;
    }
    if s.len() > 2 {
        return "US";
    }

    s
}

#[allow(unused)]
#[cfg(test)]
mod test {

    use std::env::set_var;

    use crate::{route::maxmind::filter_iso_string_to_iso3166, COMMON_DIRS};

    use super::get_ip_iso;

    /// see  doc/notes.md, Country.mmdb is required
    //#[test]
    fn test1() {
        set_var("RUST_LOG", "debug");

        use tracing_subscriber::{fmt, prelude::*, EnvFilter};
        let _ = tracing_subscriber::registry()
            .with(EnvFilter::from_default_env())
            .with(fmt::layer().with_writer(std::io::stderr))
            .try_init();

        let s = get_ip_iso("127.0.0.1".parse().unwrap(), "Country.mmdb", &COMMON_DIRS);
        println!("{s}");
        assert_eq!(s, "PRIVATE");
        assert_eq!(filter_iso_string_to_iso3166(&s), "PRIVATE");

        //www.baidu.com's IP
        let s = get_ip_iso(
            "104.193.88.123".parse().unwrap(),
            "Country.mmdb",
            &COMMON_DIRS,
        );
        println!("{s}");
        assert_eq!(s, "CN");
        assert_eq!(filter_iso_string_to_iso3166(&s), "CN");

        // www.google.com's IP
        let s = get_ip_iso(
            "142.251.32.36".parse().unwrap(),
            "Country.mmdb",
            &COMMON_DIRS,
        );
        println!("{s}");
        assert_eq!(s, "GOOGLE");
        assert_eq!(filter_iso_string_to_iso3166(&s), "US");

        // www.twitter.com's IP
        let s = get_ip_iso(
            "104.244.42.1".parse().unwrap(),
            "Country.mmdb",
            &COMMON_DIRS,
        );
        println!("{s}");
        assert_eq!(s, "TWITTER");

        // www.reddit.com's IP
        let s = get_ip_iso(
            "151.101.65.140".parse().unwrap(),
            "Country.mmdb",
            &COMMON_DIRS,
        );
        println!("{s}");
        assert_eq!(s, "FASTLY");
        assert_eq!(filter_iso_string_to_iso3166(&s), "US");
    }
}
