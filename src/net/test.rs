use std::net::IpAddr;
use std::net::ToSocketAddrs;

use crate::net::gen_random_higher_port;

use super::*;

const TEST_DOMAIN: &str = "www.baidu.com";

#[test]
fn print_cidchain() {
    let cc = CIDChain {
        id_list: vec![1, 2, 3],
    };
    println!("{}", cc)
}

#[test]
fn randomport() {
    let x = gen_random_higher_port();
    println!("{:?}", x);
}

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

    let a = Addr::from_name_network_addr_str("ip://10.0.0.1:24#utun111").unwrap();
    println!("{:?}", a);

    let r = a.to_name_ip_netmask();
    println!("{:?}", r);
    assert!(r.is_ok());

    let a = Addr::from_name_network_addr_str("ip://10.0.0.1:24").unwrap();
    println!("{:?}", a);

    let r = a.to_name_ip_netmask();
    println!("{:?}", r);
    assert!(r.is_ok());
}
