use std::io::Read;

use anyhow::bail;
use rucimp::{utils::FileSource, DEFAULT_LUA_CONFIG_FILE_NAME};
use tracing::debug;

/*
! mode 模块对应 rucimp 中的 mode 模块。
*/
pub mod chain;

pub async fn get_file(
    file_name: &mut String,
    in_memory: bool,
) -> anyhow::Result<(String, FileSource)> {
    use anyhow::Context;

    let get_file_f = || -> anyhow::Result<_> {
        rucimp::utils::try_get_file_content(DEFAULT_LUA_CONFIG_FILE_NAME, Some(&file_name))
            .with_context(|| format!("run chain engine try get file {} failed", file_name))
    };

    //获取到文件的 bytes, 或通过下载 或读取文件. 若 in_memory 给出则下载的文件不持久化

    let mut file_bytes_v = if file_name.starts_with("http://") || file_name.starts_with("https://")
    {
        #[cfg(feature = "utils")]
        {
            let url: String = file_name.to_string();

            file_name.replace_range(.., url.split('/').last().unwrap());

            match in_memory {
                true => crate::utils::dl_url(&url, None).await?.unwrap(),
                false => {
                    let _ = crate::utils::dl_url(&url, Some(&file_name)).await?;

                    let mut v = vec![];

                    let mut file = std::fs::File::open(&file_name)?;
                    file.read_to_end(&mut v)?;

                    v
                }
            }
        }

        #[cfg(not(feature = "utils"))]
        {
            get_file_f()?
        }
    } else {
        get_file_f()?
    };

    // zip, tar, lua/toml 三种情况. zip 要解压
    // 之后若为 tar, 则会将 Engine 的 FileSource 设为 该tar, 后续 Engine 访问文件都会只在该tar 中寻找

    if file_name.ends_with(".zip") {
        let real_fn = file_name.strip_suffix(".zip").unwrap_or(file_name);

        file_bytes_v = rucimp::utils::extract_vec_from_zip(real_fn, file_bytes_v)?;
        *file_name = real_fn.to_string();
    }

    let mut file_source = FileSource::default();

    if file_name.ends_with(".tar") {
        let tar_file_bytes_v = file_bytes_v;
        let md5_s = format!(
            "{:x}",
            rucimp::utils::md5::compute(tar_file_bytes_v.as_slice())
        );

        let should_be = file_name.split_once('.').unwrap().0;

        if should_be != md5_s {
            bail!(
                "md5 do not match: should be {}, but got {}",
                should_be,
                md5_s
            );
        } else {
            debug!("md5 match")
        }

        //在 tar 的情况下，约定所使用的 配置文件 名称只能为 local.lua 或 local.toml
        let mut real_file_bytes_r =
            rucimp::utils::get_file_from_tar(DEFAULT_LUA_CONFIG_FILE_NAME, &tar_file_bytes_v);

        if real_file_bytes_r.is_err() {
            real_file_bytes_r = rucimp::utils::get_file_from_tar("local.toml", &tar_file_bytes_v);
        }
        let real_file_bytes = real_file_bytes_r?;

        file_source = FileSource::Tar(tar_file_bytes_v);
        file_bytes_v = real_file_bytes;

        let real_fn = file_name.strip_suffix(".tar").unwrap_or(file_name);
        *file_name = real_fn.to_string();
    }

    let contents = String::from_utf8_lossy(file_bytes_v.as_slice()).to_string();

    Ok((contents, file_source))
}
#[cfg(test)]
mod test {
    use ruci::net::dns::{self, ClientConfig};
    use rucimp::modes::chain::config::PlainTextSet;
    use rucimp::modes::chain::config::{
        DirectConfig, InMapConfig, InMapConfigChain, OutMapConfig, OutMapConfigChain, StaticConfig,
    };
    use std::collections::HashMap;

    #[test]
    fn serialize_toml() {
        let sa = std::net::SocketAddr::V4("114.114.114.114:53".parse().unwrap());
        let sc = StaticConfig {
            inbounds: vec![InMapConfigChain {
                tag: None,
                chain: vec![
                    InMapConfig::Listener {
                        listen_addr: "0.0.0.0:1080".to_string(),
                        ext: None,
                    },
                    InMapConfig::Counter,
                    InMapConfig::Socks5(PlainTextSet {
                        userpass: None,
                        more: None,
                    }),
                ],
            }],
            outbounds: vec![OutMapConfigChain {
                tag: String::from("todo!()"),
                chain: vec![
                    OutMapConfig::Direct(DirectConfig { dns_client: None }),
                    OutMapConfig::Direct(DirectConfig {
                        dns_client: Some(ClientConfig {
                            dns_server_list: vec![(sa, dns::TheProtocol::Udp)],
                            ip_strategy: Some(dns::TheLookupIpStrategy::Ipv4Only),
                            static_pairs: HashMap::new(),
                        }),
                    }),
                ],
            }],
            ..Default::default()
        };
        let toml = toml::to_string(&sc).expect("valid toml");
        println!("{:#}", toml);

        let toml: StaticConfig = toml::from_str(&toml).expect("valid toml");
        println!("{:#?}", toml);
    }
}
