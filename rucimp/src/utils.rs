/*!
Provides some helper functions to read a certain resource file or to wait the shutdown signal.
*/
use std::io::Read;

use anyhow::{anyhow, Context};
use tokio::signal;
use tracing::{debug, info};

use crate::COMMON_DIRS;

#[derive(Clone, Debug)]
pub enum FileSource {
    Folders(Vec<String>), //从指定的一组路径来寻找文件
    Tar(Vec<u8>),         // 从一个 已放到内存中的 tar 中 寻找文件
}
impl Default for FileSource {
    fn default() -> Self {
        FileSource::Folders(COMMON_DIRS.iter().map(|str| str.to_string()).collect())
    }
}

impl FileSource {
    /// 返回读到的 数据。如果 source 为 Folders ， 则还会返回 成功找到的路径
    pub fn get_file_content<'a>(
        &'a self,
        file_name: &'a str,
    ) -> anyhow::Result<(Vec<u8>, Option<&'a str>)> {
        match self {
            FileSource::Tar(v) => get_file_from_tar(file_name, v).map(|x| (x, None)),

            FileSource::Folders(possible_addrs) => {
                for dir in possible_addrs {
                    let real_file_name = String::from(dir) + file_name;

                    if std::path::Path::new(&real_file_name).exists() {
                        if let Ok(mut file) = std::fs::File::open(real_file_name) {
                            let mut v = vec![];
                            file.read_to_end(&mut v)?;

                            return Ok((v, Some(dir)));
                        }
                    }
                }
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "not found").into())
            }
        }
    }
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

    let fs = FileSource::default();
    let r = fs.get_file_content(filename).context("get file failed")?;

    let mut cd = std::env::current_dir().expect("has current directory");

    cd.push(r.1.unwrap());

    if cd.exists() {
        std::env::set_current_dir(cd).expect("set_current_dir ok");
        debug!("set current dir to {:?}", std::env::current_dir());
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

pub fn get_file_from_tar(file_name: &str, b: &Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let mut a = tar::Archive::new(std::io::Cursor::new(b));

    let tp = std::path::Path::new(file_name);

    debug!("finding {}, {}", file_name, b.len());

    let mut e = a
        .entries()
        .unwrap()
        .find(|a| a.as_ref().is_ok_and(|b| b.path().is_ok_and(|c| c == tp)))
        .ok_or_else(|| anyhow!("get_file_from_tar: can't find the file, {}", file_name))??;

    debug!("found {}", file_name);

    let mut v = vec![];
    use std::io::Read;
    e.read_to_end(&mut v)?;
    Ok(v)
}
