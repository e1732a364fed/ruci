local h2_config = {
  type = "H2Single",
  is_grpc = true,
  http_config = {
    path = "/service1/Tun",
    authority = "myhost"
  }
}

local outbound_h2_trojan = {
  { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
  {
    type = "TLS", host = "www.1234.com", insecure = true
  },
  h2_config,
  { type = "Trojan",     password = "mypassword" }
}

local inbound_socks_http = {
  { type = "Listener",  listen_addr = "0.0.0.0:10800" },
  { type = "Socks5Http" }
}

Config = {
  outbounds = { dial1 = outbound_h2_trojan },
  inbounds = { listen1 = inbound_socks_http }
}
