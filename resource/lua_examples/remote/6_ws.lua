local outbound_direct = {
  { type = "Direct" },
}

local http_filter_config = {
  type = "HttpFilter",
  path = "/path1",
  authority = "myhost"
}

local inbound_ws_tls_trojan = {
  { type = "Listener", listen_addr = "0.0.0.0:10801" },
  {
    type = "TLS",
    key = "test2.key",
    cert = "test2.crt",
    alpn = { "h2", "http/1.1" }
  },
  http_filter_config,
  { type = "WebSocket" },
  { type = "Trojan",   password = "mypassword" }
}

Config = {
  outbounds = { dial1 = outbound_direct },
  inbounds = { listen1 = inbound_ws_tls_trojan }
}
