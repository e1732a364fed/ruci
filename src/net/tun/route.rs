use tracing::info;

use crate::utils::sync_run_command_list_stop;

pub fn auto_route(
    tun_dev_name: &str,
    tun_gateway: &str,
    router_ip: &str,
    router_name: &str,
    direct_list: Vec<&str>,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        info!("tun up auto route for linux...");

        let list = format!(
            r#"ip route del default
ip route add default via {tun_gateway} dev {tun_dev_name} metric 1
ip route add default via {router_ip} dev {router_name} metric 10"#,
        );
        let mut list: Vec<_> = list.split('\n').map(String::from).collect();

        for v in direct_list {
            list.push(format!(
                "ip route add {v} via {router_ip} dev {router_name} metric 10"
            ))
        }

        sync_run_command_list_stop(list.iter().map(String::as_str).collect())?;

        // run_command("ip", &format!("tuntap add mode tun dev {tun_dev_name}",))?;
        // run_command(
        //     "ip",
        //     &format!("addr add {tun_gateway} /15 dev {tun_dev_name}",),
        // )?;

        // run_command("ip", &format!("link set dev {tun_dev_name} up",))?;
    }

    Ok(())
}

pub fn down_route(
    router_ip: &str,
    router_name: &str,
    direct_list: Vec<&str>,
) -> anyhow::Result<()> {
    let mut list = vec![];

    #[cfg(target_os = "linux")]
    {
        info!("tun down auto route for linux...");

        for v in direct_list {
            list.push(format!(
                "ip route add {v} via {router_ip} dev {router_name} metric 10"
            ))
        }
        sync_run_command_list_stop(list.iter().map(String::as_str).collect())?;
    }
    Ok(())
}
