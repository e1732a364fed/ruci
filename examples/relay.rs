use std::sync::Arc;

use ruci::{
    map::{
        socks5http::{self, Config},
        MapBox,
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

    let socks5hmap = socks5http::Server::new(Config::default()).await;

    let socks5box: MapBox = Box::new(socks5hmap);

    let vec = vec![Arc::new(socks5box)];

    let inbounds = ruci::map::fold::DynVecIterWrapper(vec.into_iter());

    let direct = ruci::map::network::Direct::default();

    let direct_box: MapBox = Box::new(direct);

    let outbounds_vec = vec![Arc::new(direct_box)];

    let outbounds = ruci::map::fold::DynVecIterWrapper(outbounds_vec.into_iter());

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
