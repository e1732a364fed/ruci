use log::debug;

use tun2::{AsyncDevice, IntoAddress};

use crate::Name;

use super::Conn;

impl Name for AsyncDevice {
    fn name(&self) -> &str {
        "tun_conn"
    }
}

pub async fn dial<A1, A2>(
    tun_name: Option<String>,
    dial_addr: A1,
    netmask: A2,
) -> anyhow::Result<Conn>
where
    A1: IntoAddress,
    A2: IntoAddress,
{
    let mut config = tun2::Configuration::default();

    config
        .tun_name(tun_name.as_ref().map(String::as_str).unwrap_or("utun321"))
        .address(dial_addr)
        .netmask(netmask)
        .up();

    #[cfg(target_os = "linux")]
    config.platform_config(|config| {
        config.ensure_root_privileges(true);
    });

    let dev = tun2::create_as_async(&config).unwrap();

    debug!("tun: dial succeed");

    Ok(Box::new(dev))
}

#[cfg(test)]
#[allow(unused)]
mod test {
    use tokio::io::AsyncReadExt;

    use crate::net::Addr;

    use super::dial;

    //sudo -E cargo test --package ruci --lib --features tun -- net::tun::test::test --exact --nocapture
    //#[tokio::test]
    async fn test() {
        let a = Addr::from_strs("ip", "utun432", "10.0.0.1", 24).unwrap();
        let (dn, ip, nm) = a.to_name_ip_netmask().unwrap();
        let mut conn = dial(dn, ip, nm).await.unwrap();
        let mut buf = [0; 4096];
        println!("reading...\nuse:\nsudo ifconfig utun432 10.0.0.1 10.0.0.2 up\non macos, then \nping 10.0.0.2");
        let amount = conn.read(&mut buf).await.unwrap();
        println!("{:?}", &buf[0..amount]);
    }
}
