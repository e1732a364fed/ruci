use std::net::IpAddr;

use maxminddb::geoip2;

pub fn get_ip_iso(ip: IpAddr) -> String {
    let reader = maxminddb::Reader::open_readfile("resource/Country.mmdb").unwrap();

    let c: geoip2::Country = reader.lookup(ip).unwrap();
    c.country.unwrap().iso_code.unwrap().to_string()
}
//todo: use real file; cache in mem; remove unwrap; add test
