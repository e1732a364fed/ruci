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
  chain = {
    {
      type = "OptDialer",
      sockopt = sockopt_config,
      dial_addr = "tcp://192.168.0.204:10801"

    },
    {
      type = "TLS", host = "www.1234.com", insecure = true
    },
    ws_config
  },
  tag = "dial1"
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

local inbound_tun = {
  chain = { tun_config },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_ws_tls },
  inbounds = { inbound_tun }
}
