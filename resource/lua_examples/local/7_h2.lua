local h2_config = {
  is_grpc = true,
  http_config = {
    path = "/service1/Tun",
    authority = "myhost"
  }
}

local outbound_h2_trojan = {
  chain = {
    { BindDialer = { dial_addr = "tcp://127.0.0.1:10801" } },
    {
      TLS = {
        host = "www.1234.com",
        insecure = true
      }
    },
    { H2Single = h2_config },
    { Trojan = { password = "mypassword" } }
  },
  tag = "dial1"
}

local inbound_socks_http = {
  chain = {
    { Listener = { listen_addr = "0.0.0.0:10800" } },
    { Socks5Http = {} }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_h2_trojan },
  inbounds = { inbound_socks_http }
}
