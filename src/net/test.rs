use std::net::IpAddr;
use std::net::ToSocketAddrs;

use super::Addr;

const TEST_DOMAIN: &str = "www.baidu.com";

#[test]
#[should_panic]
fn try_parseip_fromstr() {
    use std::str::FromStr;
    let x = IpAddr::from_str(TEST_DOMAIN);
    println!("{}", x.as_ref().err().unwrap());
    x.unwrap();
}

#[test]
fn try_resolvehost() {
    let x = (TEST_DOMAIN.to_string() + ":80").to_socket_addrs();
    println!("{:?}", x);
    x.unwrap();
}

#[test]
fn addr_to_name_ip_netmask() {
    let a = Addr::from_strs("ip", "utun432", "10.0.0.1", 24).unwrap();
    println!("{:?}", a);

    let r = a.to_name_ip_netmask();
    println!("{:?}", r);
    assert!(r.is_ok());

    let a = Addr::from_strs("ip", "", "10.0.0.1", 24).unwrap();
    println!("{:?}", a);

    let r = a.to_name_ip_netmask();
    println!("{:?}", r);
    assert!(r.is_ok());
}
