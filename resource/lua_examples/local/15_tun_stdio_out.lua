local outbound_stdio = {
  type = "Stdio", write_mode = "Bytes"
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


Config = {
  outbounds = { dial1 = { outbound_stdio } },
  inbounds = { listen1 = { tun_config } }
}
