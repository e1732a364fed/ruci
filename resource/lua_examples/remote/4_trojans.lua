local outbound_direct = {
  chain = { { Direct = {} } },
  tag = "dial1"
}

local outbound_fallback = {
  chain = { {
    BindDialer = { dial_addr = "tcp://0.0.0.0:80" }
  } },
  tag = "fallback_d"
}

local inbound_tls_trojan = {
  chain = {
    { Listener = { listen_addr = "0.0.0.0:10801" } },
    {
      TLS = {
        key = "test2.key",
        cert = "test2.crt",
        alpn = { "h2", "http/1.1" }
      }
    },
    { Trojan = { password = "mypassword" } }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_direct, outbound_fallback },
  inbounds = { inbound_tls_trojan },
  fallback_route = { { "listen1", "fallback_d" } }
}
