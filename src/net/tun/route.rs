use anyhow::Context;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{
    net::dns::{get_sys_dns, set_sys_dns},
    utils::{self, sync_run_command_list_stop},
};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct InAutoRouteParams {
    pub tun_dev_name: Option<String>,
    pub tun_gateway: Option<String>,
    pub router_ip: Option<String>,
    pub original_dev_name: Option<String>,
    pub direct_list: Option<Vec<String>>,
    pub dns_list: Option<Vec<String>>,
}

// const DEFAULT_ROUTER_IP: &str = "192.168.0.1";
const DEFAULT_ORIGINAL_DEV_NAME: &str = "enp0s1";

pub fn in_auto_route(params: &InAutoRouteParams) -> anyhow::Result<Option<Vec<String>>> {
    #[cfg(target_os = "linux")]
    {
        info!("tun up auto route for linux...");

        let tun_gateway = params.tun_gateway.as_deref().unwrap_or("10.0.0.1");
        let tun_dev_name = params.tun_dev_name.as_deref().unwrap_or("utun321");
        //let router_ip = params.router_ip.as_deref().unwrap_or(DEFAULT_ROUTER_IP);
        let original_dev_name = params
            .original_dev_name
            .as_deref()
            .unwrap_or(DEFAULT_ORIGINAL_DEV_NAME);

        // 有些只有一个网卡的设备是没有 default 路由的, 此时 运行 下面命令会报错.
        //  因此该命令的成败不影响大局
        let _r = utils::run_command("ip", "route del default");

        let list =
            format!(r#"ip route add default via {tun_gateway} dev {tun_dev_name} metric 1"#,);

        //ip route add default via {router_ip} dev {original_dev_name} metric 10

        let mut list: Vec<_> = list.split('\n').map(String::from).collect();

        if let Some(direct_list) = &params.direct_list {
            for v in direct_list.iter() {
                list.push(format!(
                    "ip route add {v} dev {original_dev_name} metric 100"
                ))
            }
        }

        let r = sync_run_command_list_stop(list.iter().map(String::as_str).collect());

        if let Err(e) = r {
            warn!("auto_route run command got e, will down_route: {}", e);

            let _ = in_down_route(params);
            return Err(e);
        }

        if let Some(d) = &params.dns_list {
            let old_dns_list = get_sys_dns();
            let d = d.iter().map(String::as_str).collect();
            set_sys_dns(d).context("set_sys_dns failed")?;
            return Ok(Some(old_dns_list));
        }
    }

    Ok(None)
}

pub fn in_down_route(params: &InAutoRouteParams) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        info!("tun down auto route for linux...");
        let mut list = vec![];

        //let router_ip = params.router_ip.as_deref().unwrap_or(DEFAULT_ROUTER_IP);
        let original_dev_name = params
            .original_dev_name
            .as_deref()
            .unwrap_or(DEFAULT_ORIGINAL_DEV_NAME);

        if let Some(direct_list) = &params.direct_list {
            for v in direct_list {
                list.push(format!(
                    "ip route del {v} dev {original_dev_name} metric 100"
                ))
            }
        }

        sync_run_command_list_stop(list.iter().map(String::as_str).collect())?;

        if let Some(d) = &params.dns_list {
            let d = d.iter().map(String::as_str).collect();
            set_sys_dns(d).context("set_sys_dns failed")?;
        }
    }
    Ok(())
}
