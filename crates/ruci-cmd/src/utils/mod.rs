use std::{fs, sync::Arc, time::Duration};

use anyhow::{Context, Ok};
use clap::Subcommand;
use ruci::net;
use serde::Deserialize;
use serde_value::Value;
use tokio::sync::mpsc;
use tracing::info;

pub const WINTUN_DOWNLOAD_LINK: &str = "https://www.wintun.net/builds/wintun-0.14.1.zip";

pub const MMDB_DOWNLOAD_LINK: &str =
    "https://cdn.jsdelivr.net/gh/Loyalsoldier/geoip@release/Country.mmdb";

pub const RUCI_WEBUI_DOWNLOAD_LINK: &str =
    "https://github.com/e1732a364fed/ruci-webui/releases/latest/download/dist.tar.gz";

// 运行示例： ruci-cmd utils convert-format local.lua json

#[derive(Subcommand, Clone)]
pub enum Commands {
    /// download Country.mmdb
    Mmdb,

    /// download wintun.zip
    Wintun,

    Webui,

    /// calculate trojan hash for a plain text password
    CalcuTrojanHash {
        password: String,
    },

    /// generate self signed root certificate and key
    GenCer {
        subject_alt_names: Vec<String>,
    },

    //CA证书一定是自签名的
    /// generate CA certificate and key
    GenCA {
        organization_name: Option<String>,

        common_name: Option<String>,

        subject_alt_names: Vec<String>,
    },

    /// start a interactive lua shell, which is a read–eval–print loop (REPL).
    #[cfg(any(feature = "lua", feature = "lua54"))]
    Repl,

    /// pack a folder into a .tar file, calculate its md5 hash and use it as the file name.
    Pack {
        folder: String,
    },

    /// pack a folder into a .tar file, calculate its md5 hash and use it as the file name, then compress it into a .zip file.
    ///
    /// 注意 hash 仍为 tar 为 md5 而不是 zip 的 md5
    PackZ {
        folder: String,
    },

    /// print the QrCode of a string in the console.
    QR {
        str: String,
    },

    /// 转换配置文件格式，支持在 lua、json 之间互相转换。输入格式将根据文件后缀自动识别
    ConvertFormat {
        /// 输入文件路径
        input_file: String,
        /// 输出格式 (lua/json)
        output_format: String,
    },
}

pub async fn deal_cmds(command: Option<Commands>) -> anyhow::Result<()> {
    let cmd = match command {
        Some(c) => c,
        None => return Ok(()),
    };
    match cmd {
        Commands::Webui => {
            download_webui().await?;
        }
        Commands::Mmdb => {
            download_mmdb().await?;
        }
        Commands::Wintun => {
            download_wintun().await?;
        }
        Commands::CalcuTrojanHash { password } => print_calcu_trojan_hash(&password),
        Commands::GenCA {
            subject_alt_names,
            organization_name,
            common_name,
        } => generate_ca_certificate(subject_alt_names, organization_name, common_name)?,
        Commands::GenCer {
            subject_alt_names: names,
        } => generate_certificate(names)?,
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

        Commands::QR { str } => print_qrcode_of(&str),
        Commands::ConvertFormat {
            input_file,
            output_format,
        } => convert_format(input_file, output_format).await?,
    };
    Ok(())
}

pub async fn convert_format_with_content(
    input_file_name: String,
    input_file_content: String,
    data_source: data_source::DataSource,
    output_format: String,
) -> anyhow::Result<()> {
    // 从文件名获取输入格式
    let input_format = input_file_name
        .rsplit('.')
        .next()
        .context("无法从文件名获取格式")?
        .to_lowercase();
    let output = convert_static_config(
        &input_file_content,
        &input_format,
        &output_format,
        data_source,
    )
    .context("convert_static_config")?;

    let ifp = std::path::Path::new(&input_file_name);

    let x = ifp.parent().unwrap_or(std::path::Path::new("")).join(
        ifp.file_stem()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or(input_file_name.clone()),
    );

    let mut output_file = format!("{}.{}", x.as_path().to_str().unwrap(), output_format);

    // 如果文件已存在，则在文件名后添加数字
    let mut counter = 1;
    while fs::metadata(&output_file).is_ok() {
        output_file = format!(
            "{}_{}.{}",
            input_file_name
                .rsplit('.')
                .nth(1)
                .unwrap_or(&input_file_name),
            counter,
            output_format
        );
        counter += 1;
    }

    fs::write(&output_file, &output).context(format!("fs::write {output_file}"))?;
    info!("配置已转换并保存至: {}", output_file);

    Ok(())
}

pub async fn convert_format(
    mut input_file_name: String,
    output_format: String,
) -> anyhow::Result<()> {
    let (input_file_contents, data_source) =
        crate::mode::chain::get_config_file(&mut input_file_name, false)
            .await
            .context(format!("failed to read file: {}", input_file_name))?;

    convert_format_with_content(
        input_file_name,
        input_file_contents,
        data_source,
        output_format,
    )
    .await
}

fn generate_ca_certificate(
    subject_alt_names: Vec<String>,
    organization_name: Option<String>,
    common_name: Option<String>,
) -> anyhow::Result<()> {
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
    Ok(())
}

fn generate_certificate(names: Vec<String>) -> anyhow::Result<()> {
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

fn calcu_trojan_hash_fn(plain_text: &str) -> String {
    ruci::map::trojan::sha224_hex_string_lower_case(plain_text)
}

fn print_calcu_trojan_hash(plain_text: &str) {
    let h = calcu_trojan_hash_fn(plain_text);
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

async fn download_webui() -> anyhow::Result<()> {
    const FILENAME: &str = "webui.tar.gz";
    dl_url(RUCI_WEBUI_DOWNLOAD_LINK, Some(FILENAME)).await?;

    use flate2::read::GzDecoder;
    use std::fs::File;
    use tar::Archive;

    let tar_gz = File::open(FILENAME)?;
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);
    archive.unpack(".")?;

    Ok(())
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

fn qrcode_of(str: &str) -> String {
    use qrcode::render::unicode;
    use qrcode::QrCode;
    let code = QrCode::new(str).unwrap();
    let image_str = code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();
    image_str
}

fn print_qrcode_of(str: &str) {
    let image_str = qrcode_of(str);
    println!("{image_str}");
}

/// ruci模式下 在不同配置格式之间转换
/// 支持的格式: lua, json
///
/// # Arguments
/// * `input` - 输入的配置文件内容
/// * `input_format` - 输入格式 ("lua", "json")
/// * `output_format` - 输出格式 ("lua", "json")
pub fn convert_static_config(
    input_file_content: &str,
    input_format: &str,
    output_format: &str,
    data_source: data_source::DataSource,
) -> anyhow::Result<String> {
    use rucimp::modes::chain::config::StaticConfig;

    // 处理 lua 格式的特殊情况
    if input_format.to_lowercase() == "lua" {
        #[cfg(any(feature = "lua", feature = "lua54"))]
        {
            let config: StaticConfig = rucimp::modes::chain::config::lua::load_static(
                input_file_content,
                Arc::new(data_source),
            )
            .context("init_lua_static failed")?;

            info!("{:?}", config);

            return match output_format.to_lowercase().as_str() {
                // "toml" => Ok(toml::to_string(&config)?),
                "json" => Ok(rucimp::serde_json::to_string_pretty(&config)?),
                // "yaml" | "yml" => Ok(serde_yaml::to_string(&config)?),
                _ => anyhow::bail!("unsupported output format: {}", output_format),
            };
        }
        #[cfg(not(any(feature = "lua", feature = "lua54")))]
        anyhow::bail!("lua feature not enabled");
    }

    if output_format.to_lowercase() == "lua" {
        #[cfg(any(feature = "lua", feature = "lua54"))]
        {
            let config: StaticConfig = match input_format.to_lowercase().as_str() {
                // "toml" => toml::from_str(input_file_content)?,
                "json" => rucimp::serde_json::from_str(input_file_content)?,
                // "yaml" | "yml" => serde_yaml::from_str(input_file_content)?,
                _ => anyhow::bail!("unsupported input format: {}", input_format),
            };
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
            return Ok(s);
        }
        #[cfg(not(any(feature = "lua", feature = "lua54")))]
        anyhow::bail!("lua feature not enabled");
    }

    // 首先将输入解析为 serde_value::Value
    let value: Value = match input_format.to_lowercase().as_str() {
        "json" => rucimp::serde_json::from_str(input_file_content).context("json parse failed")?,
        // "yaml" | "yml" => serde_yaml::from_str(input_file_content).context("yaml parse failed")?,
        // "toml" => toml::from_str(input_file_content).context("toml parse failed")?,
        _ => anyhow::bail!("unsupported input format: {}", input_format),
    };

    // 然后将 Value 序列化为目标格式
    match output_format.to_lowercase().as_str() {
        "json" => {
            Ok(rucimp::serde_json::to_string_pretty(&value)?).context("serialize json failed")
        }
        // "yaml" | "yml" => Ok(serde_yaml::to_string(&value)?).context("serialize yaml failed"),
        // "toml" => Ok(toml::to_string(&value)?).context("serialize toml failed"),
        _ => anyhow::bail!("unsupported output format: {}", output_format),
    }
}

#[cfg(feature = "api_server")]
pub fn register_command_apis(
    api_extensions: &mut rucimp::api::ApiExtensionMap,
) -> anyhow::Result<()> {
    use axum::routing::{get, post};
    use tracing::debug;

    let mut extensions = api_extensions.write();

    extensions.insert(
        "/api/utils/generate_ca_certificate/{name}".to_string(),
        get(
            |axum::extract::Path(name): axum::extract::Path<String>| async {
                let r = generate_ca_certificate(vec![name], None, None);
                format!("{r:?}")
            },
        ),
    );

    extensions.insert(
        "/api/utils/generate_certificate/{name}".to_string(),
        get(
            |axum::extract::Path(name): axum::extract::Path<String>| async {
                let r = generate_certificate(vec![name]);
                format!("{r:?}")
            },
        ),
    );

    extensions.insert(
        "/api/utils/download/webui".to_string(),
        get(|| async {
            let r = download_webui().await;
            format!("{r:?}")
        }),
    );

    extensions.insert(
        "/api/utils/download/mmdb".to_string(),
        get(|| async {
            let r = download_wintun().await;
            format!("{r:?}")
        }),
    );

    extensions.insert(
        "/api/utils/download/wintun".to_string(),
        get(|| async {
            let r = download_mmdb().await;
            format!("{r:?}")
        }),
    );

    extensions.insert(
        "/api/utils/trojan_hash/{password}".to_string(),
        get(
            |axum::extract::Path(password): axum::extract::Path<String>| async move {
                calcu_trojan_hash_fn(&password)
            },
        ),
    );

    extensions.insert(
        "/api/utils/qr/{text}".to_string(),
        get(
            |axum::extract::Path(text): axum::extract::Path<String>| async move {
                qrcode_of(&text).to_string()
            },
        ),
    );

    #[derive(Deserialize)]
    pub struct ConvertFormatRequest {
        pub input_file_name: String,
        pub output_format: String,
    }

    extensions.insert(
        "/api/utils/convert_format/name".to_string(),
        post(
            |axum::Json(params): axum::Json<ConvertFormatRequest>| async move {
                let input_file = params.input_file_name;
                let output_format = params.output_format;

                if input_file.is_empty() || output_format.is_empty() {
                    return "错误: 缺少必要参数 input_file 或 output_format".to_string();
                }

                let r = convert_format(input_file, output_format).await;
                format!("{r:?}")
            },
        ),
    );

    #[derive(Deserialize)]
    pub struct ConvertFormatRequestByContent {
        pub input_file_name: String,
        pub input_file_content: String,
        pub output_format: String,
    }

    extensions.insert(
        "/api/utils/convert_format/content".to_string(),
        post(
            |axum::Json(params): axum::Json<ConvertFormatRequestByContent>| async move {
                let input_file = params.input_file_name;
                let input_file_c = params.input_file_content;
                let output_format = params.output_format;

                if input_file.is_empty() || output_format.is_empty()|| input_file_c.is_empty(){
                    return "错误: 缺少必要参数 input_file_name 或 input_file_content 或 output_format".to_string();
                }

                let r = convert_format_with_content(input_file,input_file_c, rucimp::utils::default_file_source(), output_format).await;
                format!("{r:?}")
            },
        )
        ,
    );

    debug!("utils: Registered {} command APIs", extensions.len());

    Ok(())
}
