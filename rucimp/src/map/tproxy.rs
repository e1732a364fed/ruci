/*!
Tproxy related Mapper. Tproxy is shortcut for transparent proxy,

Only support linux
 */
use std::process::Command;

use anyhow::Context;
use async_trait::async_trait;
use ruci::map::{self, *};
use ruci::{net::*, Name};

use macro_mapper::{mapper_ext_fields, MapperExt};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::utils::run_command_list;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Options {
    pub port: Option<u32>,
    pub auto_route: Option<bool>,
    pub auto_route_tcp: Option<bool>,
}

/// TproxyResolver 从 系统发来的 tproxy 相关的 连接
/// 解析出实际 target_addr
#[mapper_ext_fields]
#[derive(Debug, Clone, Default, MapperExt)]
pub struct TproxyResolver {
    //opts: Options,
    port: Option<u32>,
}

impl Name for TproxyResolver {
    fn name(&self) -> &'static str {
        "tproxy_resolver"
    }
}

fn run_tcp_route(port: u32) -> anyhow::Result<()> {
    Command::new("ip")
        .args(["rule", "add", "fwmark", "1", "table", "100"])
        .spawn()?;

    Command::new("ip")
        .args("route add local 0.0.0.0/0 dev lo table 100".split(' '))
        .spawn()?;

    //rucimp , proxy other devices
    // rucimp_self , proxy self

    Command::new("iptables")
        .args(["-t", "mangle", "-N", "rucimp"])
        .spawn()?;

    Command::new("iptables")
        .args("-t mangle -A rucimp -d 127.0.0.1/32 -j RETURN".split(' '))
        .spawn()?;
    Command::new("iptables")
        .args("-t mangle -A rucimp -d 224.0.0.0/4 -j RETURN".split(' '))
        .spawn()?;
    Command::new("iptables")
        .args("-t mangle -A rucimp -d 255.255.255.255/32 -j RETURN".split(' '))
        .spawn()?;

    Command::new("iptables")
        .args("-t mangle -A rucimp -d 192.168.0.0/16 -p tcp -j RETURN".split(' '))
        .spawn()?;

    Command::new("iptables")
        .args(
            format!("-t mangle -A rucimp -p tcp -j TPROXY --on-port {port} --tproxy-mark 1")
                .split(' '),
        )
        .spawn()?;

    Command::new("iptables")
        .args("-t mangle -A PREROUTING -j rucimp".split(' '))
        .spawn()?;

    Command::new("iptables")
        .args(["-t", "mangle", "-N", "rucimp_self"])
        .spawn()?;

    Command::new("iptables")
        .args("-t mangle -A rucimp_self -d 224.0.0.0/4 -j RETURN".split(' '))
        .spawn()?;

    Command::new("iptables")
        .args("-t mangle -A rucimp_self -d 255.255.255.255/32 -j RETURN".split(' '))
        .spawn()?;

    Command::new("iptables")
        .args("-t mangle -A rucimp_self -d 192.168.0.0/16 -p tcp -j RETURN".split(' '))
        .spawn()?;

    Command::new("iptables")
        .args("-t mangle -A rucimp_self -j RETURN -m mark --mark 0xff".split(' '))
        .spawn()?;

    Command::new("iptables")
        .args("-t mangle -A rucimp_self -p tcp -j MARK --set-mark 1".split(' '))
        .spawn()?;

    Command::new("iptables")
        .args("-t mangle -A OUTPUT -j rucimp_self".split(' '))
        .spawn()?;

    Ok(())
}

fn down_auto_route(port: u32) -> anyhow::Result<()> {
    let list = format!(
        r#"ip rule del fwmark 1 table 100
ip route del local 0.0.0.0/0 dev lo table 100
iptables -t mangle -D rucimp -d 127.0.0.1/32 -j RETURN
iptables -t mangle -D rucimp -d 224.0.0.0/4 -j RETURN
iptables -t mangle -D rucimp -d 255.255.255.255/32 -j RETURN
iptables -t mangle -D rucimp -d 192.168.0.0/16 -p tcp -j RETURN
iptables -t mangle -D rucimp -d 192.168.0.0/16 -p udp ! --dport 53 -j RETURN
iptables -t mangle -D rucimp -p udp -j TPROXY --on-port {port} --tproxy-mark 1
iptables -t mangle -D rucimp -p tcp -j TPROXY --on-port {port} --tproxy-mark 1
iptables -t mangle -D PREROUTING -j rucimp
iptables -t mangle -D rucimp_self -d 224.0.0.0/4 -j RETURN
iptables -t mangle -D rucimp_self -d 255.255.255.255/32 -j RETURN
iptables -t mangle -D rucimp_self -d 192.168.0.0/16 -p tcp -j RETURN
iptables -t mangle -D rucimp_self -d 192.168.0.0/16 -p udp ! --dport 53 -j RETURN
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
    run_command_list(list)?;

    Ok(())
}

impl TproxyResolver {
    pub fn new(opts: Options) -> anyhow::Result<Self> {
        if opts.auto_route_tcp.unwrap_or_default() {
            info!("tproxy run auto_route_tcp");

            run_tcp_route(opts.port.unwrap_or(12345))
                .context("run auto_route_tcp commands failed")?;
        }
        Ok(Self {
            //opts: opts.clone(),
            port: opts.port,
            ext_fields: Some(MapperExtFields::default()),
        })
    }
}

impl Drop for TproxyResolver {
    fn drop(&mut self) {
        info!("tproxy run down_auto_route");

        let r = down_auto_route(self.port.unwrap_or(12345));
        if let Err(e) = r {
            warn!("tproxy run down_auto_route got error {e}")
        }
    }
}

fn get_laddr_from_vd(vd: Vec<Option<Box<dyn Data>>>) -> Option<ruci::net::Addr> {
    for vd in vd.iter().flatten() {
        let oa = vd.get_laddr();
        if oa.is_some() {
            return oa;
        }
    }
    None
}

#[async_trait]
impl Mapper for TproxyResolver {
    ///tproxy only has decode behavior
    ///
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::Conn(c) => {
                let oa = get_laddr_from_vd(params.d);

                if oa.is_none() {
                    return MapResult::err_str(
                        "Tproxy needs data for local_addr, did't get it from the data.",
                    );
                }
                debug!(cid = %cid, a=?oa, "tproxy got target_addr: ");

                // laddr in tproxy is in fact target_addr
                MapResult::new_c(c).a(oa).b(params.b).build()
            }
            Stream::AddrConn(_) => todo!(),
            _ => MapResult::err_str(&format!("Tproxy needs a stream, got {}", params.c)),
        }
    }
}
