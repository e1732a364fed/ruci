local ws_config = {
  type = "WebSocket",
  path = "/path1",
  use_early_data = true,
  authority = "myhost"
}

local outbound_ws_trojan = {
  { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
  {
    type = "TLS",
    host = "www.1234.com",
    insecure = true
  },
  ws_config,
  { type = "Trojan",     password = "mypassword" }
}

local inbound_socks_http = {
  { type = "Listener",  listen_addr = "0.0.0.0:10800" },
  { type = "Socks5Http" }
}

Config = {
  outbounds = { dial1 = outbound_ws_trojan },
  inbounds = { listen1 = inbound_socks_http }
}
