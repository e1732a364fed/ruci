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
  {
    type = "Direct",
    dns_client = dns_config
  },
}

local inbound_socks_http =
{
  { type = "Listener",  listen_addr = "0.0.0.0:10800" },
  { type = "Socks5Http" }
}

local inbound_dns_proxy = {
  {
    type = "Listener",
    listen_addr = "udp://0.0.0.0:20800",
    ext = { fixed_target_addr = "udp://8.8.8.8:53" }

  },
}

Config = {
  outbounds = { dial1 = outbound_direct_dns },
  inbounds = { listen1 = inbound_socks_http, listen2 = inbound_dns_proxy }
}
