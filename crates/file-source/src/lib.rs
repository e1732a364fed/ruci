use std::io;

use tracing::debug;
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
                    let real_file_name = std::path::Path::new(dir).join(file_name.as_ref());

                    if real_file_name.exists() {
                        return std::fs::read(&real_file_name).map(|v| (v, Some(dir.as_str())));
                    }
                }
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "File not found in specified directories: {:?}",
                        file_name.as_ref()
                    ),
                ))
            }
            FileSource::StdReadFile => {
                let s = std::fs::read_to_string(file_name)?;
                Ok((s.into_bytes(), None))
            }
        }
    }
}

pub fn get_file_from_tar<P>(file_name_in_tar: P, tar_binary: &Vec<u8>) -> io::Result<Vec<u8>>
where
    P: AsRef<std::path::Path>,
{
    let mut a = tar::Archive::new(std::io::Cursor::new(tar_binary));

    debug!(
        "finding {} from tar, tar whole size is {}",
        file_name_in_tar.as_ref().to_str().unwrap(),
        tar_binary.len()
    );

    let mut e = a
        .entries()
        .unwrap()
        .find(|a| {
            a.as_ref()
                .is_ok_and(|b| b.path().is_ok_and(|c| c == file_name_in_tar.as_ref()))
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "get_file_from_tar: can't find the file, {}",
                    file_name_in_tar.as_ref().to_str().unwrap()
                ),
            )
        })??;

    debug!("found {}", file_name_in_tar.as_ref().to_str().unwrap());

    let mut result = vec![];
    use std::io::Read;
    e.read_to_end(&mut result)?;
    Ok(result)
}
