local sockopt_config = {
  bind_to_device = "wlp3s0",
  so_mark = 255
}

local outbound_opt_direct = {
  {
    type = "OptDirect",
    sockopt = sockopt_config,
    more_num_of_files = true
  },
}

local inbound_tls_trojan = {
  { type = "Listener", listen_addr = "0.0.0.0:10801" },
  {
    type = "TLS",
    key = "test2.key",
    cert = "test2.crt",
    alpn = { "h2", "http/1.1" }
  },
  { type = "Trojan",   password = "mypassword" }
}

Config = {
  outbounds = { dial1 = outbound_opt_direct },
  inbounds = { listen1 = inbound_tls_trojan },
  routes = {
    fallback_route = { { "listen1", "fallback_d" } }

  }
}
