local sockopt_config = {
  bind_to_device = "wlp3s0",
  so_mark = 255
}

local outbound_opt_direct = {
  chain = { {
    OptDirect = {
      sockopt = sockopt_config,
      more_num_of_files = true
    }
  } },
  tag = "dial1"
}

local inbound_tls_trojan = {
  chain = {
    { Listener = { listen_addr = "0.0.0.0:10801" } },
    {
      TLS = {
        key = "test2.key",
        cert = "test2.crt",
        alpn = { "h2", "http/1.1" }
      }
    },
    { Trojan = { password = "mypassword" } }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_opt_direct },
  inbounds = { inbound_tls_trojan },
  fallback_route = { { "listen1", "fallback_d" } }
}
