/*! mode 模块对应 rucimp 中的 mode 模块。
*/

use std::path::Path;

use anyhow::bail;
use data_source::{DataSource, SyncFolderSource};
use rucimp::DEFAULT_LUA_CONFIG_FILE_NAME;
use tracing::debug;

pub mod chain;

#[allow(unused)]
pub async fn get_config_file(
    file_name: &mut String,
    in_memory: bool,
) -> anyhow::Result<(String, DataSource)> {
    use anyhow::Context;

    let mut data_source = rucimp::utils::default_file_source();
    data_source
        .insert_current_working_dir()
        .context("insert_current_working_dir failed")?;

    let get_file_f = || -> anyhow::Result<_> {
        let mut r = data_source.get_file_content(Path::new(&file_name));

        if r.is_err() {
            r = data_source.get_file_content(Path::new(DEFAULT_LUA_CONFIG_FILE_NAME));
        }

        Ok(r.context("get_config_file data_source.get_file_content failed")?)
    };

    //获取到文件的 bytes, 或通过下载 或读取文件. 若 in_memory 给出则下载的文件不持久化

    let (mut file_bytes_v, found_dir) =
        if file_name.starts_with("http://") || file_name.starts_with("https://") {
            #[cfg(feature = "utils")]
            {
                use std::io::Read;

                let url: String = file_name.to_string();

                file_name.replace_range(.., url.split('/').last().unwrap());

                match in_memory {
                    true => (crate::utils::dl_url(&url, None).await?.unwrap(), None),
                    false => {
                        let _ = crate::utils::dl_url(&url, Some(file_name)).await?;

                        let mut v = vec![];

                        let mut file = std::fs::File::open(file_name.clone())?;
                        file.read_to_end(&mut v)?;

                        (v, None)
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
    // 之后若为 tar, 则会将 Engine 的 DataSource 设为 该tar, 后续 Engine 访问文件都会只在该tar 中寻找

    if file_name.to_lowercase().ends_with(".zip") {
        let real_fn = file_name.strip_suffix(".zip").unwrap_or(file_name);

        file_bytes_v = rucimp::utils::extract_vec_from_zip(real_fn, file_bytes_v)?;
        *file_name = real_fn.to_string();
    }

    if file_name.to_lowercase().ends_with(".tar") {
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

        use data_source::get_file_from_tar_in_memory;

        //在 tar 的情况下，约定所使用的 配置文件 名称只能为 local.lua或 local.json
        let mut real_file_bytes_r =
            get_file_from_tar_in_memory(DEFAULT_LUA_CONFIG_FILE_NAME, &tar_file_bytes_v);

        // if real_file_bytes_r.is_err() {
        //     real_file_bytes_r = get_file_from_tar("local.toml", &tar_file_bytes_v);
        // }

        if real_file_bytes_r.is_err() {
            real_file_bytes_r = get_file_from_tar_in_memory("local.json", &tar_file_bytes_v);
        }

        let (real_file_bytes, _) = real_file_bytes_r.context("get_file_from_tar failed")?;

        data_source = DataSource::TarInMemory(tar_file_bytes_v);
        file_bytes_v = real_file_bytes;

        let real_fn = file_name.strip_suffix(".tar").unwrap_or(file_name);
        *file_name = real_fn.to_string();
    }

    let contents = String::from_utf8_lossy(file_bytes_v.as_slice()).to_string();

    Ok((contents, data_source))
}
#[cfg(test)]
mod test {
    use ruci::net::dns::{self, ClientConfig};
    use rucimp::modes::chain::config::PlainTextPassSet;
    use rucimp::modes::chain::config::{
        DirectConfig, InMapConfig, InMapConfigChain, OutMapConfig, OutMapConfigChain, StaticConfig,
    };
    use rucimp::serde_json;

    #[test]
    fn serialize() {
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
                    InMapConfig::Socks5(PlainTextPassSet::default()),
                ],
            }],
            outbounds: vec![OutMapConfigChain {
                tag: String::from("todo!()"),
                chain: vec![
                    OutMapConfig::Direct(DirectConfig::default()),
                    OutMapConfig::Direct(DirectConfig {
                        dns_client: Some(ClientConfig {
                            dns_server_list: vec![(sa, dns::TheProtocol::Udp)],
                            ip_strategy: Some(dns::TheLookupIpStrategy::Ipv4Only),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                ],
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&sc).expect("valid json");
        println!("{:#}", json);

        let sc: StaticConfig = serde_json::from_str(&json).expect("valid json");
        println!("{:#?}", sc);
    }
}
