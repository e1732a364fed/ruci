local outbound_direct = {
  { type = "Direct" },
}

local outbound_fallback = {
  {
    type = "BindDialer", dial_addr = "tcp://0.0.0.0:80"
  },
}

local inbound_tls_trojan = {
  { type = "Listener", listen_addr = "0.0.0.0:10801" },
  {
    type = "TLS",
    key = "test2.key",
    cert = "test2.crt",
    alpn = { "h2", "http/1.1" }
  },
  { type = "Trojan",   password = "mypassword" }
}

Config = {
  outbounds = { dial1 = outbound_direct, fallback_d = outbound_fallback },
  inbounds = { listen1 = inbound_tls_trojan },
  routes = {
    fallback_route = { { "listen1", "fallback_d" } }

  }
}
