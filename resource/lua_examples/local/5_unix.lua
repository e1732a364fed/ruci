local outbound_unix_trojan = {
  chain = {
    { BindDialer = { dial_addr = "unix://file1" } },
    {
      TLS = {
        host = "www.1234.com",
        insecure = true
      }
    },
    { Trojan = "mypassword" }
  },
  tag = "dial1"
}

local inbound_socks_http = {
  chain = {
    { Listener = { listen_addr = "0.0.0.0:10800" } },
    { Socks5Http = {} }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_unix_trojan },
  inbounds = { inbound_socks_http }
}
