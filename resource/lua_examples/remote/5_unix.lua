local outbound_direct = {
  chain = { { Direct = {} } },
  tag = "dial1"
}

local inbound_unix_tls_trojan = {
  chain = {
    { Listener = { listen_addr = "unix://file1" } },
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
  outbounds = { outbound_direct },
  inbounds = { inbound_unix_tls_trojan }
}
