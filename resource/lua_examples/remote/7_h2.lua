local outbound_direct = {
  { type = "Direct" },
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
  { type = "Listener", listen_addr = "0.0.0.0:10801" },
  {
    type = "TLS",
    key = "test2.key",
    cert = "test2.crt",
    alpn = { "h2", "http/1.1" }
  },
  h2_config,
  { type = "Trojan",   password = "mypassword" }
}

Config = {
  outbounds = { dial1 = outbound_direct },
  inbounds = { listen1 = inbound_h2_tls_trojan }
}
