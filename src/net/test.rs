use std::net::IpAddr;
use std::net::ToSocketAddrs;

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
