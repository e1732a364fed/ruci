use std::{fs, sync::Arc, time::Duration};

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

// 运行示例： ruci-cmd utils convert-format local.lua toml

#[derive(Subcommand, Clone)]
pub enum Commands {
    /// download Country.mmdb
    Mmdb,

    /// download wintun.zip
    Wintun,

    /// calculate trojan hash for a plain text password
    CalcuTrojanHash { password: String },

    /// generate self signed root certificate and key
    GenCer { subject_alt_names: Vec<String> },
    GenCA {
        organization_name: Option<String>,

        common_name: Option<String>,

        subject_alt_names: Vec<String>,
    },

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
    ServeStatic { addr: Option<String> },

    /// print the QrCode of a string in the console.
    QR { str: String },

    /// 转换配置文件格式，支持在 lua、toml、yaml 之间互相转换。输入格式将根据文件后缀自动识别
    ConvertFormat {
        /// 输入文件路径
        input_file: String,
        /// 输出格式 (toml/yaml)
        output_format: String,
    },
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
        Commands::GenCA {
            subject_alt_names,
            organization_name,
            common_name,
        } => {
            let on = organization_name.unwrap_or("My Company".to_string());
            let cn = common_name.unwrap_or("My CA Root".to_string());

            info!("generating CA cert and key... with {on} as OrganizationName and {cn} as CommonName");

            if !subject_alt_names.is_empty() {
                info!("and with subject_alt_names: {:?}", subject_alt_names);
            }

            use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair};
            use std::fs;

            let mut params = CertificateParams::new(subject_alt_names)?;

            params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            params.distinguished_name.push(DnType::OrganizationName, on);
            params.distinguished_name.push(DnType::CommonName, cn);

            let key_pair = KeyPair::generate()?;
            let cert = params.self_signed(&key_pair)?;

            let key_file_name = "ca_private_key.pem";

            fs::write(key_file_name, key_pair.serialize_pem())?;

            let cert_file_name = "ca_cert.pem";

            fs::write(cert_file_name, cert.pem())?;

            info!("generated {key_file_name} as {cert_file_name}");
        }
        Commands::GenCer {
            subject_alt_names: names,
        } => {
            info!("generating cert and key...");

            use rcgen::generate_simple_self_signed;

            let cert = generate_simple_self_signed(names)?;
            let c = cert.key_pair.serialize_pem();

            let key_file_name = "generated.key";

            fs::write(key_file_name, c)?;
            info!("generated key as {key_file_name}");

            let c = cert.cert.pem();

            let cert_file_name = "generated.crt";

            fs::write(cert_file_name, c)?;

            info!("generated cert as {cert_file_name}");
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
        Commands::ServeStatic { addr } => {
            folder_serve::serve_static(addr).await;

            let _ = rucimp::utils::wait_close_sig().await;
        }
        Commands::QR { str } => print_qrcode_of(&str),
        Commands::ConvertFormat {
            mut input_file,
            output_format,
        } => {
            let (contents, file_source) = mode::get_file(&mut input_file, false)
                .await
                .context(format!("failed to read file: {}", input_file))?;

            // 从文件名获取输入格式
            let input_format = input_file
                .rsplit('.')
                .next()
                .context("无法从文件名获取格式")?
                .to_lowercase();

            let output = convert_config(&contents, &input_format, &output_format, file_source)?;

            let mut output_file = format!(
                "{}.{}",
                input_file.rsplit('.').nth(1).unwrap_or(&input_file),
                output_format
            );

            // 如果文件已存在，则在文件名后添加数字
            let mut counter = 1;
            while fs::metadata(&output_file).is_ok() {
                output_file = format!(
                    "{}_{}.{}",
                    input_file.rsplit('.').nth(1).unwrap_or(&input_file),
                    counter,
                    output_format
                );
                counter += 1;
            }

            fs::write(&output_file, output)?;
            info!("配置已转换并保存至: {}", output_file);
        }
    };
    Ok(())
}

/// print a line of info and pack the folder into tar
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
    println!("{image}");
}

/// 在不同配置格式之间转换
/// 支持的格式: lua, toml, yaml
///
/// # Arguments
/// * `input` - 输入的配置文件内容
/// * `input_format` - 输入格式 ("lua", "toml", "yaml")
/// * `output_format` - 输出格式 ("lua", "toml", "yaml")
pub fn convert_config(
    input_file_content: &str,
    input_format: &str,
    output_format: &str,
    file_source: ruci::utils::FileSource,
) -> anyhow::Result<String> {
    use rucimp::modes::chain::config::StaticConfig;

    // 首先将输入解析为 StaticConfig
    let config: StaticConfig = match input_format.to_lowercase().as_str() {
        "lua" => {
            #[cfg(any(feature = "lua", feature = "lua54"))]
            {
                rucimp::modes::chain::config::lua::load_static(
                    input_file_content,
                    Arc::new(file_source),
                )
                .context("init_lua_static failed")?
            }
            #[cfg(not(any(feature = "lua", feature = "lua54")))]
            anyhow::bail!("lua feature not enabled")
        }
        "toml" => toml::from_str(input_file_content)?,
        "yaml" | "yml" => serde_yaml::from_str(input_file_content)?,
        _ => anyhow::bail!("unsupported input format: {}", input_format),
    };

    // 然后将 StaticConfig 转换为目标格式
    match output_format.to_lowercase().as_str() {
        "toml" => Ok(toml::to_string(&config)?),
        "yaml" | "yml" => Ok(serde_yaml::to_string(&config)?),
        "lua" => {
            #[cfg(any(feature = "lua", feature = "lua54"))]
            {
                use rucimp::modes::chain::config::lua::mlua::{self, LuaSerdeExt};
                let lua = mlua::Lua::new();
                let lua_value = lua.to_value(&config)?;

                let s = rucimp::modes::chain::config::lua::lua_value_to_string_with_prefix(
                    &lua_value,
                    "Config = ",
                )?;

                let s = s
                    .lines()
                    .filter(|s| {
                        let s = s.trim_end();
                        !(s.ends_with("= nil,") || s.contains("= nil"))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(s)
            }
            #[cfg(not(any(feature = "lua", feature = "lua54")))]
            anyhow::bail!("lua feature not enabled")
        }
        _ => anyhow::bail!("unsupported output format: {}", output_format),
    }
}
