use crate::suit::SuitConfigHolder;

use super::config::Config;

use super::SuitStruct;

#[test]
fn replace_empty_string() {
    let mut s = String::new();
    s.replace_range(.., "direct");
    assert_eq!(s, "direct");
}

#[test]
fn init_suit() {
    let toml_str = r#"
    [[listen]]
    protocol = "socks5"
    host = "127.0.0.1"
    port = 12345
    uuid = "u0 p0"
    users = [ { user = "u1", pass = "p1"},  { user = "u2", pass = "p2"}, ]

    [[dial]]
    protocol = "direct"
    "#;
    let mut c: Config = toml::from_str(toml_str).unwrap();
    println!("{:#?}", c);

    let mut lsuit = SuitStruct::from(c.listen.pop().unwrap());
    lsuit.set_behavior(ruci::map::ProxyBehavior::DECODE);

    println!("{:?}", lsuit);

    let csuit = SuitStruct::from(c.dial.pop().unwrap());
    println!("{:?}", csuit);
}
