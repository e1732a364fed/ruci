local tls_alpn = { "h2", "http/1.1" }

local outbound_trojan = {
  { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
  {
    type = "TLS",
    host = "www.google.com",
    insecure = true,
    alpn = tls_alpn

  },
  { type = "Trojan",     password = "mypassword" }
}

local mitm_config = {
  type = "MITM",
  key = "test_ca_key.pem",
  cert = "test_ca_cert.pem",
  alpn = tls_alpn
}

local inbound_mitm = {
  { type = "Listener",  listen_addr = "0.0.0.0:10800" },
  { type = "Socks5Http" },
  mitm_config
}

Config = {
  outbounds = { dial1 = outbound_trojan },
  inbounds = { listen1 = inbound_mitm }
}
