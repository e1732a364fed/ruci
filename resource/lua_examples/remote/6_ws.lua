local outbound_direct = {
  chain = { { type = "Direct" } },
  tag = "dial1"
}

local http_filter_config = {
  type = "HttpFilter",
  path = "/path1",
  authority = "myhost"
}

local inbound_ws_tls_trojan = {
  chain = {
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
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_direct },
  inbounds = { inbound_ws_tls_trojan }
}
