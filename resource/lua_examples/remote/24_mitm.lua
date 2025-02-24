local tls_alpn = { "h2", "http/1.1" }

local outbound_mitm = {
  chain = {
    { type = "Direct", leak_target_addr = true },
    {

      type = "TLS",
      insecure = false,
      alpn = tls_alpn

    }
  },
  tag = "dial1"
}

local outbound_fallback = {
  chain = {
    { type = "BindDialer", dial_addr = "tcp://0.0.0.0:4433" }
  },
  tag = "fallback_d"
}

local inbound_tls_trojan = {
  chain = {
    { type = "Listener", listen_addr = "0.0.0.0:10801" },
    {
      type = "TLS",
      key = "test2.key",
      cert = "test2.crt",
      alpn = tls_alpn

    },
    { type = "Trojan",   password = "mypassword" }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_mitm, outbound_fallback },
  inbounds = { inbound_tls_trojan },
  fallback_route = { { "listen1", "fallback_d" } }
}
