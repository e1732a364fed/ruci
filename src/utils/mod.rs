pub mod record;

use std::{fmt, io, process::Command};

use anyhow::bail;
use bytes::BytesMut;
use tracing::{debug, trace, warn};

/// remove first character, and return the trimmed str
pub fn rm_first(value: &str) -> &str {
    let mut chars = value.chars();
    chars.next();
    chars.as_str()
}

/// bytes to Captalized hex string(like 1234FF)
pub struct HexSlice<'a>(pub &'a [u8]);

impl fmt::Display for HexSlice<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self.0.len();
        write!(f, "{:06},", len)?;

        for byte in self.0 {
            write!(f, "{:02X}", byte)?;
        }
        writeln!(f)?;
        Ok(())
    }
}

/// generate an io::ErrorKind::Other
pub fn io_error<T: std::fmt::Display>(message: T) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("{}", message))
}

/// generate an io::ErrorKind::Other
pub fn io_error2<T: std::fmt::Display, T2: std::fmt::Display>(
    message: T,
    message2: T2,
) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("{} {}", message, message2))
}

pub fn buf_to_ob(b: BytesMut) -> Option<BytesMut> {
    if b.is_empty() {
        None
    } else {
        Some(b)
    }
}

pub fn run_command(cmd: &str, args: &str) -> anyhow::Result<()> {
    trace!(cmd = cmd, args = ?args, "running command",);

    let r = Command::new(cmd).args(args.split(' ')).output()?;

    if r.status.success() {
        Ok(())
    } else {
        bail!("err output: {:?}", r);
    }
}

/// keep run next command if got error
pub fn sync_run_command_list_no_stop(list: Vec<&str>, no_warn: bool) -> anyhow::Result<()> {
    //debug!("utils: start run_command_list ");
    for cmd in list {
        let mut strs: Vec<_> = cmd.split(' ').collect();
        if strs.is_empty() {
            bail!("got empty command");
        }
        let args = strs.split_off(1);

        trace!(cmd = strs[0], args = ?args, "running command",);

        let r = Command::new(strs[0]).args(args).output();
        match r {
            Ok(o) => {
                if !o.status.success() {
                    if !no_warn {
                        warn!("run command not success, result is {:?}", o);
                    }
                    continue;
                }
            }
            Err(e) => {
                if !no_warn {
                    warn!("run command got err, result is {:?}", e);
                }
                continue;
            }
        }
    }
    //debug!("utils: finish run_command_list ");

    Ok(())
}

/// stop run if got error
pub fn sync_run_command_list_stop(list: Vec<&str>) -> anyhow::Result<()> {
    //debug!("utils: start run_command_list ");
    for cmd in list {
        let mut strs: Vec<_> = cmd.split(' ').collect();
        if strs.is_empty() {
            bail!("got empty command");
        }
        let args = strs.split_off(1);

        trace!(cmd = strs[0], args = ?args, "running command",);

        let r = Command::new(strs[0]).args(args).output();

        match r {
            Ok(o) => {
                if !o.status.success() {
                    bail!("run command not success, result is {:?}", o);
                }
            }
            Err(e) => {
                warn!("run command got err, result is {:?}", e);
                return Err(e.into());
            }
        }
    }
    //debug!("utils: finish run_command_list ");

    Ok(())
}

/// Defines where to get the content of the requested file name.
#[derive(Clone, Debug, Default)]
pub enum FileSource {
    #[default]
    StdReadFile,
    Folders(Vec<String>), //从指定的一组路径来寻找文件

    #[cfg(feature = "tar")]
    Tar(Vec<u8>), // 从一个 已放到内存中的 tar 中 寻找文件
}

impl FileSource {
    pub fn insert_current_working_dir(&mut self) -> io::Result<()> {
        if let FileSource::Folders(ref mut v) = self {
            v.push(std::env::current_dir()?.to_string_lossy().to_string())
        }
        Ok(())
    }

    pub fn read_to_string<'a, P>(&'a self, file_name: P) -> io::Result<String>
    where
        P: AsRef<std::path::Path>,
    {
        let r = self.get_file_content(file_name)?;
        Ok(String::from_utf8_lossy(r.0.as_slice()).to_string())
    }

    /// 返回读到的 数据。如果 source 为 Folders ， 则还会返回 成功找到的路径
    pub fn get_file_content<'a, P>(&'a self, file_name: P) -> io::Result<(Vec<u8>, Option<&'a str>)>
    where
        P: AsRef<std::path::Path>,
    {
        match self {
            #[cfg(feature = "tar")]
            FileSource::Tar(v) => get_file_from_tar(file_name, v).map(|data| (data, None)),

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
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "not found").into())
            }
            FileSource::StdReadFile => {
                let s = std::fs::read_to_string(file_name)?;
                Ok((s.into_bytes(), None))
            }
        }
    }
}

#[cfg(feature = "tar")]
pub fn get_file_from_tar<P>(file_name: P, b: &Vec<u8>) -> io::Result<Vec<u8>>
where
    P: AsRef<std::path::Path>,
{
    let mut a = tar::Archive::new(std::io::Cursor::new(b));

    debug!(
        "finding {}, {}",
        file_name.as_ref().to_str().unwrap(),
        b.len()
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

    let mut v = vec![];
    use std::io::Read;
    e.read_to_end(&mut v)?;
    Ok(v)
}
