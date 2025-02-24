local outbound_direct = {
  chain = { { type = "Direct" } },
  tag = "dial1"
}

local h2_config = {
  type = "H2",
  is_grpc = true,
  http_config = {
    path = "/service1/Tun",
    authority = "myhost"
  }
}

local inbound_h2_tls_trojan = {
  chain = {
    { type = "Listener", listen_addr = "0.0.0.0:10801" },
    {
      type = "TLS",
      key = "test2.key",
      cert = "test2.crt",
      alpn = { "h2", "http/1.1" }
    },
    h2_config,
    { type = "Trojan",   password = "mypassword" }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_direct },
  inbounds = { inbound_h2_tls_trojan }
}
