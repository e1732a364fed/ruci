local outbound_direct = {
  chain = { { Direct = {} } },
  tag = "dial1"
}

local http_filter_config = {
  path = "/path1",
  authority = "myhost"
}

local inbound_ws_tls_trojan = {
  chain = {
    { Listener = { listen_addr = "0.0.0.0:10801" } },
    {
      TLS = {
        key = "test2.key",
        cert = "test2.crt",
        alpn = { "h2", "http/1.1" }
      }
    },
    { HttpFilter = http_filter_config },
    { WebSocket = {} },
    { Trojan = { password = "mypassword" } }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_direct },
  inbounds = { inbound_ws_tls_trojan }
}
