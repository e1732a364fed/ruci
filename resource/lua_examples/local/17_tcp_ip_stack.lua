local sockopt_config = {
  bind_to_device = "wlp3s0",
}

local outbound_opt_direct = {
  {
    type = "OptDirect", sockopt = sockopt_config
  },
}

local tun_config = {
  type = "BindDialer",
  in_auto_route = {
    tun_dev_name = "utun321",
    dns_list = { "114.114.114.114" },
    original_dev_name = "wlp3s0",
    tun_gateway = "10.0.0.1",
    router_ip = "192.168.0.1"
  },
  bind_addr = "ip://10.0.0.1:24#utun321"
}



Config = {
  inbounds = {
    listen1 = { tun_config, { type = "StackLwip" } }
  },
  outbounds = { dial1 = outbound_opt_direct }
}
