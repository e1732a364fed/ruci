local outbound_direct = {
  chain = { { type = "Direct" } },
  tag = "dial1"
}

local outbound_fallback = {
  chain = { {
    type = "BindDialer", dial_addr = "tcp://0.0.0.0:80"
  } },
  tag = "fallback_d"
}

local inbound_tls_trojan = {
  chain = {
    { type = "Listener", listen_addr = "0.0.0.0:10801" },
    {
      type = "TLS",
      key = "test2.key",
      cert = "test2.crt",
      alpn = { "h2", "http/1.1" }
    },
    { type = "Trojan",   password = "mypassword" }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_direct, outbound_fallback },
  inbounds = { inbound_tls_trojan },
  fallback_route = { { "listen1", "fallback_d" } }
}
