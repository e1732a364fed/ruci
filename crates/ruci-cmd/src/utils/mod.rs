use std::time::Duration;

use super::*;
use anyhow::{Context, Ok};
use log::info;
use ruci::net;
use tokio::sync::mpsc;

pub const WINTUN_DOWNLOAD_LINK: &str = "https://www.wintun.net/builds/wintun-0.14.1.zip";

pub const MMDB_DOWNLOAD_LINK: &str =
    "https://cdn.jsdelivr.net/gh/Loyalsoldier/geoip@release/Country.mmdb";

#[derive(Subcommand)]
pub enum Commands {
    DownloadMMDB,
    DownloadWINTUN,
}

pub async fn deal_cmds(command: Option<Commands>) -> anyhow::Result<()> {
    let cmd = match command {
        Some(c) => c,
        None => return Ok(()),
    };
    match cmd {
        Commands::DownloadMMDB => {
            download_mmdb().await?;
        }
        Commands::DownloadWINTUN => {
            download_wintun().await?;
        }
    };
    Ok(())
}

#[tokio::test]
async fn test_download_mmdb() -> anyhow::Result<()> {
    download_mmdb().await
}

//https://github.com/seanmonstar/reqwest/issues/482#issuecomment-1951347935
fn response_to_async_read(resp: reqwest::Response) -> impl tokio::io::AsyncRead {
    use futures::stream::TryStreamExt;

    let stream = resp.bytes_stream().map_err(std::io::Error::other);
    tokio_util::io::StreamReader::new(stream)
}

/// download a file from url
///
/// timeout is 10s
///
/// will print download progress during downloading.
///
pub async fn dl_url(url: &str, file_name: &str) -> anyhow::Result<()> {
    info!("try downloading {} from {} ", file_name, url);
    use bytesize::ByteSize;
    let response = tokio::time::timeout(Duration::from_secs(10), reqwest::get(url))
        .await
        .context("dl waiting for too long")??;

    info!("got response");
    let size = response.content_length().unwrap_or_default();
    let sf = size as f64;

    info!("file size is {}", ByteSize(size),);

    let mut file = tokio::fs::File::create(file_name).await?;
    let mut content = response_to_async_read(response);

    let (tx, mut rx) = mpsc::channel(10);
    tokio::spawn(async move {
        let mut i: usize = 0;
        let mut total: u64 = 0;
        loop {
            match rx.recv().await {
                Some((_, db)) => {
                    i += 1;
                    total += db;
                    let p = total as f64 / sf;
                    let p100 = p * 100 as f64;
                    print!(
                        "\r progress: {:>5.2}%; {:>5}; db +{}, total: {}; ",
                        p100,
                        i,
                        ByteSize(db),
                        ByteSize(total),
                    )
                }
                None => break,
            }
        }
    });

    let cid = net::CID::default();
    net::cp::cp_rw_with_updater(&cid, &mut content, &mut file, tx).await?;
    info!("download {file_name} succeed");

    Ok(())
}

async fn download_mmdb() -> anyhow::Result<()> {
    dl_url(MMDB_DOWNLOAD_LINK, "Country.mmdb").await?;
    info!("download succeed");
    Ok(())
}

async fn download_wintun() -> anyhow::Result<()> {
    const WINTUNZIP: &str = "wintun.zip";
    dl_url(WINTUN_DOWNLOAD_LINK, WINTUNZIP).await?;
    Ok(())
}
