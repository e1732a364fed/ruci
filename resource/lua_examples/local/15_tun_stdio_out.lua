local outbound_stdio = {
  chain = { {
    type = "Stdio", write_mode = "Bytes"
  } },
  tag = "dial1"
}

local tun_config = {
  type = "BindDialer",
  in_auto_route = {
    tun_dev_name = "utun321",
    dns_list = { "1.1.1.1" },
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
  outbounds = { outbound_stdio },
  inbounds = { inbound_tun }
}
