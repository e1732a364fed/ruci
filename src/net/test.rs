use std::net::IpAddr;
use std::net::ToSocketAddrs;
use std::str::FromStr;

use bytes::Buf;
use bytes::BytesMut;

use crate::net::gen_random_higher_port;

use super::*;

const TEST_DOMAIN: &str = "www.baidu.com";

#[test]
fn test_cid() {
    let mut c = CID::default();
    c.push_num(1);
    c.push_num(2);
    c.push_num(3);
    println!("{}", c);
    let s = c.to_string();
    let a = c.pop();
    println!("{} {a}", c);
    let a = c.pop();
    println!("{} {a}", c);
    let a = c.pop();
    println!("{} {a}", c);
    let a = c.pop();
    println!("{} {a}", c);

    let cc_new = CID::from_str(&s);
    println!("{}", cc_new.unwrap());

    let cc_new = CID::from_str("123");
    println!("{}", cc_new.unwrap());

    let cc_new = CID::from_str("1-23");
    println!("{}", cc_new.unwrap());

    let cc_new = CID::from_str("1-2-3");
    println!("{}", cc_new.unwrap());

    let cc_new = CID::from_str("123x");
    assert!(cc_new.is_err())
}

#[test]
fn random_port() {
    let x = gen_random_higher_port();
    println!("{:?}", x);
}

#[test]
#[should_panic]
fn try_parse_ip_from_str() {
    use std::str::FromStr;
    let x = IpAddr::from_str(TEST_DOMAIN);
    println!("{}", x.as_ref().err().unwrap());
    x.unwrap();
}

#[test]
fn try_resolve_host() {
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

    let a = Addr::from_name_network_addr_url("ip://10.0.0.1:24#utun111").unwrap();
    println!("{:?}", a);

    let r = a.to_name_ip_netmask();
    println!("{:?}", r);
    assert!(r.is_ok());

    let a = Addr::from_name_network_addr_url("ip://10.0.0.1:24").unwrap();
    println!("{:?}", a);

    let r = a.to_name_ip_netmask();
    println!("{:?}", r);
    assert!(r.is_ok());
}

#[test]
fn test_buf() {
    let mut buf = BytesMut::zeroed(100);
    let cap = buf.capacity();
    buf.advance(2);
    assert_eq!(buf.capacity(), cap - 2)
}
