local old_device_name = "en0" -- wlp3s0, en0, "Ethernet 5", WLAN
local sockopt_config = {
  bind_to_device = old_device_name,
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
    -- direct_list = { "224.0.0.251", "224.0.0.252", "239.255.255.250" },
    original_dev_name = old_device_name,
    tun_gateway = "10.0.0.1",
    router_ip = "192.168.0.1"
  },
  bind_addr = "ip://10.0.0.1:24#utun321"
}



Config = {
  inbounds = {
    listen1 = { tun_config, { type = "StackSmoltcp" } } --StackLwip,StackSmoltcp
  },
  outbounds = { dial1 = outbound_opt_direct }
}
