local tun_config = {
  type = "BindDialer",
  bind_addr = "ip://10.0.0.2:24#utun321",
  out_auto_route = {
    tun_dev_name = "utun321",
    original_dev_name = "enp0s1",
    router_ip = "192.168.0.1"
  }
}

local http_filter_config = {
  type = "HttpFilter",
  path = "/path1",
  authority = "myhost"
}

local outbound_tun = {
  chain = { tun_config },
  tag = "dial1"
}

local inbound_ws_tls_trojan = {
  chain = {
    { type = "Listener", listen_addr = "0.0.0.0:10801" },
    {
      type = "TLS",
      key = "test2.key",
      cert = "test2.crt",
      alpn = { "h2", "http/1.1" }
    },
    http_filter_config,
    { type = "WebSocket" },
    { type = "Trojan",   password = "mypassword" }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_tun },
  inbounds = { inbound_ws_tls_trojan }
}
