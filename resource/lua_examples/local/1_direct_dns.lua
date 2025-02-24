local dns_config = {
  static_pairs = {
    ["www.baidu.com"] = "103.235.47.188"
  },
  ip_strategy = "Ipv4Only",
  dns_server_list = {
    { "127.0.0.1:20800", "udp" }
  }
}

local outbound_direct_dns = {
  chain = { {
    type = "Direct",
    dns_client = dns_config

  } },
  tag = "dial1"
}

local inbound_socks_http = {
  chain = {
    { type = "Listener",  listen_addr = "0.0.0.0:10800" },
    { type = "Socks5Http" }
  },
  tag = "listen1"
}

local inbound_dns_proxy = {
  chain = { {
    type = "Listener",
    listen_addr = "udp://0.0.0.0:20800",
    ext = { fixed_target_addr = "udp://8.8.8.8:53" }

  } },
  tag = "listen2"
}

Config = {
  outbounds = { outbound_direct_dns },
  inbounds = { inbound_socks_http, inbound_dns_proxy }
}
