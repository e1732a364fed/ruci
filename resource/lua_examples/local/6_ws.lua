local ws_config = {
  path = "/path1",
  use_early_data = true,
  authority = "myhost"
}

local outbound_ws_trojan = {
  chain = {
    { BindDialer = { dial_addr = "tcp://127.0.0.1:10801" } },
    {
      TLS = {
        host = "www.1234.com",
        insecure = true
      }
    },
    { WebSocket = ws_config },
    { Trojan = "mypassword" }
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
  outbounds = { outbound_ws_trojan },
  inbounds = { inbound_socks_http }
}
