/*!
Defines an adapter for crate `tun` to create the tun device.

It also has a submodule 'route' for system level auto routing for the tun device.
*/

#[allow(unused)]
pub mod route;

use anyhow::Context;
use futures::stream::{SplitSink, SplitStream};
use tokio_util::codec::Framed;
use tracing::debug;

use tun::{AsyncDevice, ToAddress, TunPacketCodec};

use super::Conn;

pub fn create_bind_sink_stream<A1, A2>(
    tun_name: Option<String>,
    bind_addr: A1,
    netmask: A2,
) -> anyhow::Result<(
    SplitSink<Framed<AsyncDevice, TunPacketCodec>, Vec<u8>>,
    SplitStream<Framed<AsyncDevice, TunPacketCodec>>,
)>
where
    A1: ToAddress,
    A2: ToAddress,
{
    let device = create_bind_device(tun_name, bind_addr, netmask)?;
    let stream = device.into_framed();

    let (writer, reader) = futures::StreamExt::split(stream);
    Ok((writer, reader))
}

pub fn create_fd_device(fd: std::os::raw::c_int) -> anyhow::Result<Box<AsyncDevice>> {
    let mut cfg = tun::Configuration::default();
    cfg.raw_fd(fd);
    #[cfg(target_os = "ios")]
    cfg.platform_config(|p_cfg| {
        p_cfg.packet_information(true);
    });
    let device = tun::create_as_async(&cfg).context("create tun device failed")?;

    Ok(Box::new(device))
}

pub fn create_bind_device<A1, A2>(
    tun_name: Option<String>,
    bind_addr: A1,
    netmask: A2,
) -> anyhow::Result<Box<AsyncDevice>>
where
    A1: ToAddress,
    A2: ToAddress,
{
    let mut config = tun::Configuration::default();

    //macos only support utun{number}

    config
        .tun_name(tun_name.as_deref().unwrap_or("utun321"))
        .address(bind_addr)
        .netmask(netmask)
        .up();

    #[cfg(target_os = "linux")]
    config.platform_config(|config| {
        config.ensure_root_privileges(true);
    });

    let device = tun::create_as_async(&config).context("create tun device failed")?;

    debug!(
        tun_name = tun_name,
        dial_addr = ?config,
        "tun: create_bind succeed"
    );

    Ok(Box::new(device))
}

pub fn create_bind<A1, A2>(
    tun_name: Option<String>,
    bind_addr: A1,
    netmask: A2,
) -> anyhow::Result<Conn>
where
    A1: ToAddress,
    A2: ToAddress,
{
    let device = create_bind_device(tun_name, bind_addr, netmask)?;

    Ok(device)
}

pub fn create_bind_rw<A1, A2>(
    tun_name: Option<String>,
    bind_addr: A1,
    netmask: A2,
) -> anyhow::Result<crate::net::RW>
where
    A1: ToAddress,
    A2: ToAddress,
{
    let device = create_bind_device(tun_name, bind_addr, netmask)?;

    let wr = device.split().unwrap();

    Ok((Box::new(wr.1), Box::new(wr.0)))
}

#[cfg(test)]
#[allow(unused)]
mod test {
    use tokio::io::AsyncReadExt;

    use crate::net::Addr;

    use super::create_bind;

    //sudo -E cargo test --package ruci --lib --features tun -- net::tun::test::test --exact --nocapture
    //#[tokio::test]
    async fn test() {
        let a = Addr::from_strs("ip", "utun432", "10.0.0.1", 24).unwrap();
        let (dn, ip, nm) = a.to_name_ip_netmask().unwrap();
        let mut conn = create_bind(dn, ip, nm).unwrap();
        let mut buf = [0; 4096];
        println!("reading...\nuse:\nsudo ifconfig utun432 10.0.0.1 10.0.0.2 up\non macos, then \nping 10.0.0.2");
        let amount = conn.read(&mut buf).await.unwrap();
        println!("{:?}", &buf[0..amount]);
    }
}
