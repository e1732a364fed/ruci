local tls_alpn = { "h2", "http/1.1" }

local outbound_mitm = {
  { type = "Direct", leak_target_addr = true },
  {

    type = "TLS",
    insecure = false,
    alpn = tls_alpn

  }
}

local outbound_fallback = {
  { type = "BindDialer", dial_addr = "tcp://0.0.0.0:4433" }
}

local inbound_tls_trojan = {
  { type = "Listener", listen_addr = "0.0.0.0:10801" },
  {
    type = "TLS",
    key = "test2.key",
    cert = "test2.crt",
    alpn = tls_alpn

  },
  { type = "Trojan",   password = "mypassword" }
}

Config = {
  outbounds = { dial1 = outbound_mitm, fallback_d = outbound_fallback },
  inbounds = { listen1 = inbound_tls_trojan },
  routes = {
    fallback_route = { { "listen1", "fallback_d" } }
  }
}
