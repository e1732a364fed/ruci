use super::*;

pub const DEFAULT_LOCAL_NET: &str = "192.168.0.0/16";

/// 自动路由
///
/// 对 udp 和 tcp 执行一样的过程, 不会特别处理 udp 的 53 端口
pub fn run_tcp_route(port: u32, also_udp: bool) -> anyhow::Result<()> {
    let list = r#"ip rule add fwmark 1 table 100
ip route add local 0.0.0.0/0 dev lo table 100
iptables -t mangle -N rucimp
iptables -t mangle -A rucimp -d 127.0.0.1/32 -j RETURN
iptables -t mangle -A rucimp -d 224.0.0.0/4 -j RETURN
iptables -t mangle -A rucimp -d 255.255.255.255/32 -j RETURN
iptables -t mangle -A rucimp -d 192.168.0.0/16 -p tcp -j RETURN"#;

    let list = list.split('\n').collect_vec();

    sync_run_command_list_stop(list)?;

    //rucimp , proxy other devices
    // rucimp_self , proxy self

    if also_udp {
        run_command(
            "iptables",
            "-t mangle -A rucimp -d 192.168.0.0/16 -p udp -j RETURN",
        )?;
    }

    run_command(
        "iptables",
        format!("-t mangle -A rucimp -p tcp -j TPROXY --on-port {port} --tproxy-mark 1").as_str(),
    )?;

    if also_udp {
        run_command(
            "iptables",
            format!("-t mangle -A rucimp -p udp -j TPROXY --on-port {port} --tproxy-mark 1")
                .as_str(),
        )?;
    }

    let list = r#"iptables -t mangle -A PREROUTING -j rucimp
iptables -t mangle -N rucimp_self
iptables -t mangle -A rucimp_self -d 224.0.0.0/4 -j RETURN
iptables -t mangle -A rucimp_self -d 255.255.255.255/32 -j RETURN
iptables -t mangle -A rucimp_self -d 192.168.0.0/16 -p tcp -j RETURN"#;

    let list = list.split('\n').collect_vec();

    sync_run_command_list_stop(list)?;

    if also_udp {
        run_command(
            "iptables",
            "-t mangle -A rucimp_self -d 192.168.0.0/16 -p udp -j RETURN",
        )?;
    }

    run_command(
        "iptables",
        "-t mangle -A rucimp_self -j RETURN -m mark --mark 0xff",
    )?;

    run_command(
        "iptables",
        "-t mangle -A rucimp_self -p tcp -j MARK --set-mark 1",
    )?;

    if also_udp {
        run_command(
            "iptables",
            "-t mangle -A rucimp_self -p udp -j MARK --set-mark 1",
        )?;
    }

    //apply

    run_command("iptables", "-t mangle -A OUTPUT -j rucimp_self")?;

    Ok(())
}

pub fn down_auto_route(port: u32) -> anyhow::Result<()> {
    let list = format!(
        r#"ip rule del fwmark 1 table 100
ip route del local 0.0.0.0/0 dev lo table 100
iptables -t mangle -D rucimp -d 127.0.0.1/32 -j RETURN
iptables -t mangle -D rucimp -d 224.0.0.0/4 -j RETURN
iptables -t mangle -D rucimp -d 255.255.255.255/32 -j RETURN
iptables -t mangle -D rucimp -d 192.168.0.0/16 -p tcp -j RETURN
iptables -t mangle -D rucimp -d 192.168.0.0/16 -p udp -j RETURN
iptables -t mangle -D rucimp -p udp -j TPROXY --on-port {port} --tproxy-mark 1
iptables -t mangle -D rucimp -p tcp -j TPROXY --on-port {port} --tproxy-mark 1
iptables -t mangle -D PREROUTING -j rucimp
iptables -t mangle -D rucimp_self -d 224.0.0.0/4 -j RETURN
iptables -t mangle -D rucimp_self -d 255.255.255.255/32 -j RETURN
iptables -t mangle -D rucimp_self -d 192.168.0.0/16 -p tcp -j RETURN
iptables -t mangle -D rucimp_self -d 192.168.0.0/16 -p udp -j RETURN
iptables -t mangle -D rucimp_self -j RETURN -m mark --mark 0xff
iptables -t mangle -D rucimp_self -p udp -j MARK --set-mark 1
iptables -t mangle -D rucimp_self -p tcp -j MARK --set-mark 1
iptables -t mangle -D OUTPUT -j rucimp_self
iptables -t mangle -F rucimp
iptables -t mangle -X rucimp
iptables -t mangle -F rucimp_self
iptables -t mangle -X rucimp_self"#
    );
    let list: Vec<_> = list.split('\n').collect();
    sync_run_command_list_no_stop(list)?;

    Ok(())
}
