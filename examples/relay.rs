use std::sync::Arc;

use ruci::{
    map::{
        fold::DynVecIterWrapper,
        socks5http::{self, Config},
    },
    relay::{route::FixedOutSelector, HandleInStreamOptions},
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:10800").await?;

    let (conn, addr) = listener.accept().await?;

    println!("got tcp connection from {:?}", addr);

    let stream = ruci::net::Stream::c(Box::new(conn));

    let socks5box = socks5http::Server::boxed(Config::default());

    let inbounds: DynVecIterWrapper = vec![Arc::new(socks5box)].into();

    let direct = ruci::map::network::Direct::boxed();

    let outbounds: DynVecIterWrapper = vec![Arc::new(direct)].into();

    ruci::relay::handle_in_stream(
        stream,
        Box::new(inbounds),
        Arc::new(Box::new(FixedOutSelector {
            default: Box::new(outbounds),
        })),
        HandleInStreamOptions::default(),
    )
    .await?;

    println!("Hello, world!");

    Ok(())
}
