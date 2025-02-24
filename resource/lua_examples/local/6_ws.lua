local ws_config = {
  type = "WebSocket",
  path = "/path1",
  use_early_data = true,
  authority = "myhost"
}

local outbound_ws_trojan = {
  chain = {
    { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
    {
      type = "TLS",
      host = "www.1234.com",
      insecure = true
    },
    ws_config,
    { type = "Trojan",     password = "mypassword" }
  },
  tag = "dial1"
}

local inbound_socks_http = {
  chain = {
    { type = "Listener",  listen_addr = "0.0.0.0:10800" },
    { type = "Socks5Http" }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_ws_trojan },
  inbounds = { inbound_socks_http }
}
