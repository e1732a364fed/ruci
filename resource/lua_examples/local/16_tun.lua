local sockopt_config = {
  bind_to_device = "en0"
}

local ws_config = {
  type = "WebSocket",
  path = "/path1",
  use_early_data = true,
  authority = "myhost"
}

local outbound_ws_tls = {
  {
    type = "OptDialer",
    sockopt = sockopt_config,
    dial_addr = "tcp://192.168.0.204:10801"

  },
  {
    type = "TLS", host = "www.1234.com", insecure = true
  },
  ws_config
}

local tun_config = {
  type = "BindDialer",
  in_auto_route = {
    tun_dev_name = "utun321",
    dns_list = { "114.114.114.114" },
    original_dev_name = "enp0s1",
    tun_gateway = "10.0.0.1",
    router_ip = "192.168.0.1"
  },
  bind_addr = "ip://10.0.0.1:24#utun321"
}


Config = {
  outbounds = { dial1 = outbound_ws_tls },
  inbounds = { listen1 = { tun_config } }
}
