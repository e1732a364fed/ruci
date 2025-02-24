local outbound_direct = {
  chain = { { type = "Direct" } },
  tag = "dial1"
}

local inbound_unix_tls_trojan = {
  chain = {
    { type = "Listener", listen_addr = "unix://file1" },
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
  outbounds = { outbound_direct },
  inbounds = { inbound_unix_tls_trojan }
}
