use std::{fs, time::Duration};

use super::*;
use anyhow::{Context, Ok};
use ruci::net;
use tokio::sync::mpsc;
use tracing::info;

#[cfg(feature = "file_server")]
pub mod folder_serve;

pub const WINTUN_DOWNLOAD_LINK: &str = "https://www.wintun.net/builds/wintun-0.14.1.zip";

pub const MMDB_DOWNLOAD_LINK: &str =
    "https://cdn.jsdelivr.net/gh/Loyalsoldier/geoip@release/Country.mmdb";

#[derive(Subcommand, Clone)]
pub enum Commands {
    /// download Country.mmdb
    Mmdb,

    /// download wintun.zip
    Wintun,

    /// calculate trojan hash for a plain text password
    CalcuTrojanHash { password: String },

    /// generate self signed root certificate
    GenCer { names: Vec<String> },

    /// start a interactive lua shell, which is a read–eval–print loop (REPL).
    #[cfg(any(feature = "lua", feature = "lua54"))]
    Repl,

    /// pack a folder into a .tar file, calculate its md5 hash and use it as the file name.
    Pack { folder: String },

    /// pack a folder into a .tar file, calculate its md5 hash and use it as the file name, then compress it into a .zip file.
    ///
    /// 注意 hash 仍为 tar 为 md5 而不是 zip 的 md5
    PackZ { folder: String },

    /// serve folder "static" in plain http.
    ///
    /// default listen is "0.0.0.0:18143"
    #[cfg(feature = "file_server")]
    ServeFolder { addr: Option<String> },

    /// print the QrCode of a string in the console.
    QR { str: String },
}

pub async fn deal_cmds(command: Option<Commands>) -> anyhow::Result<()> {
    let cmd = match command {
        Some(c) => c,
        None => return Ok(()),
    };
    match cmd {
        Commands::Mmdb => {
            download_mmdb().await?;
        }
        Commands::Wintun => {
            download_wintun().await?;
        }
        Commands::CalcuTrojanHash { password } => calcu_trojan_hash(&password),
        Commands::GenCer { names } => {
            use rcgen::generate_simple_self_signed;

            let cert = generate_simple_self_signed(names).unwrap();
            let c = cert.key_pair.serialize_pem();

            fs::write("generated_crt_and_key.crt", c)?;
        }
        #[cfg(any(feature = "lua", feature = "lua54"))]
        Commands::Repl => rucimp::utils::lua_repl(),
        Commands::Pack { folder } => {
            let (v, md5) = pack_tar(&folder)?;

            write_file(v, md5)?;
        }
        Commands::PackZ { folder } => {
            let (v, mut md5) = pack_tar(&folder)?;

            info!("compressing into zip...");

            let data = rucimp::utils::compress_bytes_to_zip(&md5, &v)?;

            md5.push_str(".zip");

            write_file(data, md5)?;
        }

        #[cfg(feature = "file_server")]
        Commands::ServeFolder { addr } => {
            folder_serve::serve_static(addr).await;

            let _ = rucimp::utils::wait_close_sig().await;
        }
        Commands::QR { str } => print_qrcode_of(&str),
    };
    Ok(())
}

fn pack_tar(folder: &str) -> anyhow::Result<(Vec<u8>, String)> {
    info!("packing into tar...");
    let (bs, mut md5) = rucimp::utils::tar_folder_and_compute_md5(folder)?;
    info!("md5: {md5}");

    md5.push_str(".tar");
    Ok((bs, md5))
}
fn write_file(v: Vec<u8>, name: String) -> anyhow::Result<()> {
    let mut file = fs::File::create(&name)?;
    use std::io::Write;
    file.write_all(&v)?;
    info!("saved ok, {name}");
    Ok(())
}

fn calcu_trojan_hash(plain_text: &str) {
    let h = ruci::map::trojan::sha224_hex_string_lower_case(plain_text);
    info!("trojan hash for {plain_text} is : {h}")
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
/// will print download progress inline during downloading.
///
pub async fn dl_url(url: &str, file_name: Option<&str>) -> anyhow::Result<Option<Vec<u8>>> {
    match file_name {
        Some(file_name) => {
            info!("try downloading {file_name} from {url} ");
        }
        None => info!("try downloading {url} as bytes"),
    }
    use bytesize::ByteSize;

    const WAIT_TIME: u64 = 10;
    let response = tokio::time::timeout(Duration::from_secs(WAIT_TIME), reqwest::get(url))
        .await
        .context(format!(
            "dl waiting for too long, more than {WAIT_TIME} secs"
        ))??;

    info!("got response");
    let size = response.content_length().unwrap_or_default();
    let sf = size as f64;

    info!("file size is {}", ByteSize(size),);

    let mut content = response_to_async_read(response);

    let (tx, mut rx) = mpsc::channel(10);
    tokio::spawn(async move {
        let mut i: usize = 0;
        let mut total: u64 = 0;
        while let Some((_, db)) = rx.recv().await {
            i += 1;
            total += db;
            let p = total as f64 / sf;
            let p100 = p * 100_f64;
            print!(
                "\r progress: {:>5.2}%; {:>5}; db +{}, total: {}; ",
                p100,
                i,
                ByteSize(db),
                ByteSize(total),
            )
        }
    });

    let cid = net::CID::default();

    match file_name {
        Some(file_name) => {
            let mut file = tokio::fs::File::create(file_name).await?;
            net::cp::cp_rw_with_updater(&cid, &mut content, &mut file, tx).await?;
            info!("download {file_name} succeed");

            Ok(None)
        }
        None => {
            let mut v = vec![];
            net::cp::cp_rw_with_updater(&cid, &mut content, &mut v, tx).await?;
            info!("download succeed");

            Ok(Some(v))
        }
    }
}

async fn download_mmdb() -> anyhow::Result<()> {
    const GEOIP_COUNTRY: &str = "Country.mmdb";
    dl_url(MMDB_DOWNLOAD_LINK, Some(GEOIP_COUNTRY)).await?;
    Ok(())
}

async fn download_wintun() -> anyhow::Result<()> {
    const WINTUN_ZIP: &str = "wintun.zip";
    dl_url(WINTUN_DOWNLOAD_LINK, Some(WINTUN_ZIP)).await?;
    Ok(())
}

fn print_qrcode_of(str: &str) {
    use qrcode::render::unicode;
    use qrcode::QrCode;
    let code = QrCode::new(str).unwrap();
    let image = code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();
    println!("{}", image);
}
