use super::*;

pub const DEFAULT_LOCAL_NET4: &str = "192.168.0.0/16";

// See https://toutyrater.github.io/app/tproxy.html
// https://xtls.github.io/document/level-2/tproxy_ipv4_and_ipv6.html#%E9%A6%96%E5%85%88%E8%AE%BE%E7%BD%AE%E7%AD%96%E7%95%A5%E8%B7%AF%E7%94%B1

/// 自动路由, set route table rucimp and rucimp_self
///
/// 对 udp 和 tcp 执行一样的过程, 不会特别处理 udp 的 53 端口
pub fn run_tcp_route(port: u16, also_udp: bool, local_net4: &Option<String>) -> anyhow::Result<()> {
    let _ = down_auto_route(port, local_net4);

    let local_net4 = local_net4.as_deref().unwrap_or(DEFAULT_LOCAL_NET4);

    let list = format!(
        r#"ip rule add fwmark 1 table 100
ip route add local 0.0.0.0/0 dev lo table 100
iptables -t mangle -N rucimp
iptables -t mangle -A rucimp -d 127.0.0.1/32 -j RETURN
iptables -t mangle -A rucimp -d 224.0.0.0/4 -j RETURN
iptables -t mangle -A rucimp -d 255.255.255.255/32 -j RETURN
iptables -t mangle -A rucimp -d {local_net4} -p tcp -j RETURN"#,
    );

    let list = list.split('\n').collect_vec();

    sync_run_command_list_stop(list)?;

    //rucimp , proxy other devices
    // rucimp_self , proxy self

    if also_udp {
        run_command(
            "iptables",
            &format!("-t mangle -A rucimp -d {local_net4} -p udp -j RETURN"),
        )?;
    }

    run_command(
        "iptables",
        format!("-t mangle -A rucimp -p tcp -j TPROXY --on-ip 127.0.0.1 --on-port {port} --tproxy-mark 1").as_str(),
    )?;

    if also_udp {
        run_command(
            "iptables",
            format!("-t mangle -A rucimp -p udp -j TPROXY --on-ip 127.0.0.1 --on-port {port} --tproxy-mark 1")
                .as_str(),
        )?;
    }

    let list = format!(
        r#"iptables -t mangle -A PREROUTING -j rucimp
iptables -t mangle -N rucimp_self
iptables -t mangle -A rucimp_self -d 224.0.0.0/4 -j RETURN
iptables -t mangle -A rucimp_self -d 255.255.255.255/32 -j RETURN
iptables -t mangle -A rucimp_self -d {local_net4} -p tcp -j RETURN"#
    );

    let list = list.split('\n').collect_vec();

    sync_run_command_list_stop(list)?;

    if also_udp {
        run_command(
            "iptables",
            &format!("-t mangle -A rucimp_self -d {local_net4} -p udp -j RETURN"),
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

/// 自动路由, set route table rucimp6 and rucimp_self6
pub fn run_tcp_route6(port: u16, also_udp: bool) -> anyhow::Result<()> {
    let _ = down_auto_route6(port);

    let list = format!(
        r#"ip -6 rule add fwmark 1 table 106
ip -6 route add local ::/0 dev lo table 106
ip6tables -t mangle -N rucimp6
ip6tables -t mangle -A rucimp6 -d ::1/128 -j RETURN
ip6tables -t mangle -A rucimp6 -d fe80::/10 -j RETURN
ip6tables -t mangle -A rucimp6 -d fd00::/8 -p tcp -j RETURN"#,
    );

    let list = list.split('\n').collect_vec();

    sync_run_command_list_stop(list)?;

    if also_udp {
        run_command(
            "ip6tables",
            &format!("-t mangle -A rucimp6 -d fd00::/8 -p udp -j RETURN"),
        )?;
    }

    run_command(
        "ip6tables",
        format!(
            "-t mangle -A rucimp6 -p tcp -j TPROXY --on-ip ::1 --on-port {port} --tproxy-mark 1"
        )
        .as_str(),
    )?;

    if also_udp {
        run_command(
            "ip6tables",
            format!("-t mangle -A rucimp6 -p udp -j TPROXY --on-ip ::1 --on-port {port} --tproxy-mark 1")
                .as_str(),
        )?;
    }

    let list = format!(
        r#"ip6tables -t mangle -A PREROUTING -j rucimp6
ip6tables -t mangle -N rucimp_self6
ip6tables -t mangle -A rucimp_self6 -d fe80::/10 -j RETURN
ip6tables -t mangle -A rucimp_self6 -d fd00::/8 -p tcp -j RETURN"#
    );

    let list = list.split('\n').collect_vec();

    sync_run_command_list_stop(list)?;

    if also_udp {
        run_command(
            "ip6tables",
            &format!("-t mangle -A rucimp_self6 -d fd00::/8 -p udp -j RETURN"),
        )?;
    }

    run_command(
        "ip6tables",
        "-t mangle -A rucimp_self6 -j RETURN -m mark --mark 0xff",
    )?;

    run_command(
        "ip6tables",
        "-t mangle -A rucimp_self6 -p tcp -j MARK --set-mark 1",
    )?;

    if also_udp {
        run_command(
            "ip6tables",
            "-t mangle -A rucimp_self6 -p udp -j MARK --set-mark 1",
        )?;
    }

    //apply

    run_command("ip6tables", "-t mangle -A OUTPUT -j rucimp_self6")?;

    Ok(())
}

pub fn down_auto_route(port: u16, local_net4: &Option<String>) -> anyhow::Result<()> {
    let local_net4 = local_net4.as_deref().unwrap_or(DEFAULT_LOCAL_NET4);

    let list = format!(
        r#"ip rule del fwmark 1 table 100
ip route del local 0.0.0.0/0 dev lo table 100
iptables -t mangle -D rucimp -d 127.0.0.1/32 -j RETURN
iptables -t mangle -D rucimp -d 224.0.0.0/4 -j RETURN
iptables -t mangle -D rucimp -d 255.255.255.255/32 -j RETURN
iptables -t mangle -D rucimp -d {local_net4} -p tcp -j RETURN
iptables -t mangle -D rucimp -d {local_net4} -p udp -j RETURN
iptables -t mangle -D rucimp -p udp -j TPROXY --on-ip 127.0.0.1 --on-port {port} --tproxy-mark 1
iptables -t mangle -D rucimp -p tcp -j TPROXY --on-ip 127.0.0.1 --on-port {port} --tproxy-mark 1
iptables -t mangle -D PREROUTING -j rucimp
iptables -t mangle -D rucimp_self -d 224.0.0.0/4 -j RETURN
iptables -t mangle -D rucimp_self -d 255.255.255.255/32 -j RETURN
iptables -t mangle -D rucimp_self -d {local_net4} -p tcp -j RETURN
iptables -t mangle -D rucimp_self -d {local_net4} -p udp -j RETURN
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

pub fn down_auto_route6(port: u16) -> anyhow::Result<()> {
    let list = format!(
        r#"ip -6 rule del fwmark 1 table 106
ip -6 route del local ::/0 dev lo table 106
ip6tables -t mangle -D rucimp6 -d ::1/128 -j RETURN
ip6tables -t mangle -D rucimp6 -d fe80::/10 -j RETURN
ip6tables -t mangle -D rucimp6 -d fd00::/8 -p tcp -j RETURN
ip6tables -t mangle -D rucimp6 -d fd00::/8 -p udp -j RETURN
ip6tables -t mangle -D rucimp6 -p udp -j TPROXY --on-ip ::1 --on-port {port} --tproxy-mark 1
ip6tables -t mangle -D rucimp6 -p tcp -j TPROXY --on-ip ::1 --on-port {port} --tproxy-mark 1
ip6tables -t mangle -D PREROUTING -j rucimp6
ip6tables -t mangle -D rucimp_self6 -d fe80::/10 -j RETURN
ip6tables -t mangle -D rucimp_self6 -d fd00::/8 -p tcp -j RETURN
ip6tables -t mangle -D rucimp_self6 -d fd00::/8 -p udp -j RETURN
ip6tables -t mangle -D rucimp_self6 -j RETURN -m mark --mark 0xff
ip6tables -t mangle -D rucimp_self6 -p udp -j MARK --set-mark 1
ip6tables -t mangle -D rucimp_self6 -p tcp -j MARK --set-mark 1
ip6tables -t mangle -D OUTPUT -j rucimp_self6
ip6tables -t mangle -F rucimp6
ip6tables -t mangle -X rucimp6
ip6tables -t mangle -F rucimp_self6
ip6tables -t mangle -X rucimp_self6"#
    );
    let list: Vec<_> = list.split('\n').collect();
    sync_run_command_list_no_stop(list)?;

    Ok(())
}
