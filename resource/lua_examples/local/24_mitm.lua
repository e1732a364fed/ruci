local tls_alpn = { "h2", "http/1.1" }

local outbound_trojan = {
  chain = {
    { BindDialer = { dial_addr = "tcp://127.0.0.1:10801" } },
    {
      TLS = {
        host = "www.google.com",
        insecure = true,
        alpn = tls_alpn
      }
    },
    { Trojan = { password = "mypassword" } }
  },
  tag = "dial1"
}

local mitm_config = {
  key = "test_ca_key.pem",
  cert = "test_ca_cert.pem",
  alpn = tls_alpn
}

local inbound_mitm = {
  chain = {
    { Listener = { listen_addr = "0.0.0.0:10800" } },
    { Socks5Http = {} },
    { MITM = mitm_config }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_trojan },
  inbounds = { inbound_mitm }
}
