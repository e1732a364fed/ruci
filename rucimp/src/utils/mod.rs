/*!
Provides some helper functions to read a certain resource file or to wait the shutdown signal.
*/

pub mod anti_replay;

use std::io;

use anyhow::Context;
use tokio::signal;
use tracing::{debug, info};

use crate::COMMON_DIRS;

/// [`crate::COMMON_DIRS`]
pub fn default_file_source() -> FileSource {
    FileSource::Folders(COMMON_DIRS.iter().map(|str| str.to_string()).collect())
}

/// try folders in COMMON_DIRS
///
/// try the default_file given or the first cmd argument
///
/// and will set current dir to the directory
pub fn try_get_file_content(default_file: &str, arg_file: Option<&str>) -> anyhow::Result<Vec<u8>> {
    let filename = match arg_file.as_ref() {
        Some(a) => a,
        None => default_file,
    };
    let fs = default_file_source();
    let r = fs
        .get_file_content(filename)
        .context(format!("get file failed: {}", filename))?;

    let mut cd = std::env::current_dir().expect("has current directory");

    cd.push(r.1.unwrap());

    if cd.exists() {
        std::env::set_current_dir(cd).expect("set_current_dir ok");
        debug!("set current dir to {:?}", std::env::current_dir().unwrap());
    }

    Ok(r.0)
}

/// wait for the close signal, then log and return OK.
pub async fn wait_close_sig() -> anyhow::Result<()> {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(unix)]
    let terminate2 = async {
        signal::unix::signal(signal::unix::SignalKind::interrupt())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    #[cfg(not(unix))]
    let terminate2 = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("got ctrl_c"),
        _ = terminate => info!("got terminate"),
        _ = terminate2 => info!("got interrupt"),
    }

    info!("signal received, starting graceful shutdown...");

    Ok(())
}

/// wait for the close signal, then log and return OK.
pub async fn wait_close_sig_with_closer(
    mut c: tokio::sync::mpsc::Receiver<()>,
) -> anyhow::Result<()> {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(unix)]
    let terminate2 = async {
        signal::unix::signal(signal::unix::SignalKind::interrupt())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    #[cfg(not(unix))]
    let terminate2 = std::future::pending::<()>();

    tokio::select! {
        _ = c.recv() => info!("GOT user close"),
        _ = ctrl_c => info!("got ctrl_c"),
        _ = terminate => info!("got terminate"),
        _ = terminate2 => info!("got interrupt"),
    }

    info!("signal received, starting graceful shutdown...");

    Ok(())
}

#[cfg(all(any(feature = "lua", feature = "lua54"), feature = "repl"))]
pub fn lua_repl() {
    info!("Running lua repl. Press Ctrl+D to exit");
    //https://github.com/mlua-rs/mlua/blob/main/examples/repl.rs

    let lua = mlua::Lua::new();
    let mut editor = rustyline::DefaultEditor::new().expect("Failed to create editor");

    loop {
        let mut prompt = "> ";
        let mut line = String::new();

        loop {
            match editor.readline(prompt) {
                Ok(input) => line.push_str(&input),
                Err(_) => return,
            }

            match lua.load(&line).eval::<mlua::MultiValue>() {
                Ok(values) => {
                    editor.add_history_entry(line).unwrap();
                    println!(
                        "{}",
                        values
                            .iter()
                            .map(|value| format!("{:#?}", value))
                            .collect::<Vec<_>>()
                            .join("\t")
                    );
                    break;
                }
                Err(mlua::Error::SyntaxError {
                    incomplete_input: true,
                    ..
                }) => {
                    // continue reading input and append it to `line`
                    line.push('\n'); // separate input lines
                    prompt = ">> ";
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    break;
                }
            }
        }
    }
}

pub use md5;

pub fn tar_folder_and_compute_md5<P: AsRef<std::path::Path>>(
    src_dir: P,
) -> std::io::Result<(Vec<u8>, String)> {
    //https://crates.io/crates/tar
    let bs = vec![];

    let mut tar_builder = tar::Builder::new(bs);

    tar_builder.append_dir_all(".", src_dir)?;

    tar_builder.finish()?;

    let bs = tar_builder.into_inner()?;

    let md5_result = md5::compute(&bs);
    Ok((bs, format!("{:x}", md5_result)))
}

pub fn compress_bytes_to_zip(file_name_in_zip: &str, buf: &[u8]) -> std::io::Result<Vec<u8>> {
    // https://github.com/zip-rs/zip2/blob/master/examples/write_sample.rs

    let bs = std::io::Cursor::new(Vec::new());

    let mut zip_writer = zip::ZipWriter::new(bs);

    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    zip_writer.start_file(file_name_in_zip, options)?;
    use std::io::Write;
    zip_writer.write_all(buf)?;

    let zr = zip_writer.finish()?;
    let data = zr.into_inner();

    Ok(data)
}

pub fn extract_vec_from_zip(file_name_in_zip: &str, v: Vec<u8>) -> std::io::Result<Vec<u8>> {
    // https://github.com/zip-rs/zip2/blob/master/examples/extract_lorem.rs

    let bs = std::io::Cursor::new(v);

    let mut archive = zip::ZipArchive::new(bs).unwrap();

    let mut file = archive.by_name(file_name_in_zip)?;

    let mut v = vec![];
    use std::io::Read;
    file.read_to_end(&mut v)?;

    Ok(v)
}

/// Defines where to get the content of the requested file name.
///
/// 很多代理Map 的配置中 都要再加载其他外部文件,
/// FileSource 限定了 查找文件的 路径 和 来源, 读取文件时只会限制在这个范围内,
/// 这样就增加了安全性
#[derive(Clone, Debug, Default)]
pub enum FileSource {
    #[default]
    StdReadFile,
    Folders(Vec<String>), //从指定的一组路径来寻找文件

    Tar(Vec<u8>), // 从一个 已放到内存中的 tar 中 寻找文件
}

impl FileSource {
    pub fn insert_current_working_dir(&mut self) -> io::Result<()> {
        if let FileSource::Folders(ref mut v) = self {
            v.push(std::env::current_dir()?.to_string_lossy().to_string())
        }
        Ok(())
    }

    pub fn read_to_string<P>(&self, file_name: P) -> io::Result<String>
    where
        P: AsRef<std::path::Path>,
    {
        let r = self.get_file_content(file_name)?;
        Ok(String::from_utf8_lossy(r.0.as_slice()).to_string())
    }

    /// 返回读到的 数据。如果 source 为 Folders ， 则还会返回 成功找到的路径
    pub fn get_file_content<P>(&self, file_name: P) -> io::Result<(Vec<u8>, Option<&str>)>
    where
        P: AsRef<std::path::Path>,
    {
        match self {
            FileSource::Tar(tar_binary) => {
                get_file_from_tar(file_name, tar_binary).map(|data| (data, None))
            }

            FileSource::Folders(possible_addrs) => {
                for dir in possible_addrs {
                    let real_file_name = String::from(dir) + file_name.as_ref().to_str().unwrap();

                    // tracing::trace!("try to read file from {}", real_file_name);

                    if std::path::Path::new(&real_file_name).exists() {
                        if let Ok(mut file) = std::fs::File::open(real_file_name) {
                            let mut v = vec![];
                            use std::io::Read;
                            file.read_to_end(&mut v)?;

                            return Ok((v, Some(dir)));
                        }
                    }
                }
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not found",
                ))
            }
            FileSource::StdReadFile => {
                let s = std::fs::read_to_string(file_name)?;
                Ok((s.into_bytes(), None))
            }
        }
    }
}

pub fn get_file_from_tar<P>(file_name: P, tar_binary: &Vec<u8>) -> io::Result<Vec<u8>>
where
    P: AsRef<std::path::Path>,
{
    let mut a = tar::Archive::new(std::io::Cursor::new(tar_binary));

    debug!(
        "finding {} from tar, tar whole size is {}",
        file_name.as_ref().to_str().unwrap(),
        tar_binary.len()
    );

    let mut e = a
        .entries()
        .unwrap()
        .find(|a| {
            a.as_ref()
                .is_ok_and(|b| b.path().is_ok_and(|c| c == file_name.as_ref()))
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "get_file_from_tar: can't find the file, {}",
                    file_name.as_ref().to_str().unwrap()
                ),
            )
        })??;

    debug!("found {}", file_name.as_ref().to_str().unwrap());

    let mut result = vec![];
    use std::io::Read;
    e.read_to_end(&mut result)?;
    Ok(result)
}

/// helper function
pub fn init_tls_server_pem_option(
    opts: &ruci_tls::server::TlsServerOptions,
    fs: &FileSource,
) -> std::io::Result<ruci_tls::server::ServerPEMOptions> {
    Ok(ruci_tls::server::ServerPEMOptions {
        cert: fs.read_to_string(opts.cert.clone())?,
        key: fs.read_to_string(opts.key.clone())?,
        alpn: opts.alpn.clone(),
    })
}

/// generate an io::ErrorKind::Other
pub fn io_error2<T: std::fmt::Display, T2: std::fmt::Display>(
    message: T,
    message2: T2,
) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("{} {}", message, message2))
}
