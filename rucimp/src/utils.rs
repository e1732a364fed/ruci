/*!
Provides some helper functions to read a certain resource file or to wait the shutdown signal.
*/
use std::{fs, path::PathBuf};

use anyhow::anyhow;
use bytes::{Buf, BufMut, BytesMut};
use tokio::signal;
use tracing::{debug, info};

use crate::COMMON_DIRS;

/// try folders in COMMON_DIRS
///
/// try the default_file given or the first cmd argument
///
/// and will set current dir to the directory
pub fn try_get_file_content(default_file: &str, arg_file: Option<&str>) -> anyhow::Result<String> {
    let filename = match arg_file.as_ref() {
        Some(a) => a,
        None => default_file,
    };

    let mut last_e: Option<std::io::Error> = None;
    for dir in &COMMON_DIRS {
        let s = String::from(*dir) + filename;

        let r = fs::read_to_string(PathBuf::from(s));
        match r {
            Ok(r) => {
                let mut cd = std::env::current_dir().expect("has current directory");

                cd.push(dir);

                if cd.exists() {
                    std::env::set_current_dir(cd).expect("set_current_dir ok");
                    debug!("set current dir to {:?}", std::env::current_dir());
                }

                return Ok(r);
            }
            Err(e) => last_e = Some(e),
        }
    }

    match last_e {
        Some(e) => Err(e.into()),
        None => Err(anyhow!("open {filename} failed and no result err")),
    }
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
                    line.push_str("\n"); // separate input lines
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

pub fn tar_folder_and_compute_md5<P: AsRef<std::path::Path>>(
    src_dir: P,
) -> std::io::Result<(BytesMut, String)> {
    //https://crates.io/crates/tar
    let bs = BytesMut::with_capacity(1024 * 1024);

    let mut tar_builder = tar::Builder::new(bs.writer());

    tar_builder.append_dir_all(".", src_dir)?;

    tar_builder.finish()?;

    let bs = tar_builder.into_inner()?.into_inner();

    let md5_result = md5::compute(&bs);
    Ok((bs, format!("{:x}", md5_result)))
}

pub fn compress_bytesmut_to_zip(
    buf: &BytesMut,
    file_name_in_zip: &str,
) -> std::io::Result<Vec<u8>> {
    // https://github.com/zip-rs/zip2/blob/master/examples/write_sample.rs

    let bs = std::io::Cursor::new(Vec::new());

    let mut zip_writer = zip::ZipWriter::new(bs);

    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

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

pub fn get_file_from_tar(b: BytesMut, file_name: &str) -> anyhow::Result<Vec<u8>> {
    let mut a = tar::Archive::new(b.reader());

    let tp = std::path::Path::new(file_name);

    let mut e = a
        .entries()
        .unwrap()
        .find(|a| a.as_ref().is_ok_and(|b| b.path().is_ok_and(|c| c == tp)))
        .ok_or_else(|| anyhow!("can't find the file"))??;

    let mut v = vec![];
    use std::io::Read;
    e.read_to_end(&mut v)?;
    Ok(v)
}
