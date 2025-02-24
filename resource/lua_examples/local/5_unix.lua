local outbound_unix_trojan = {
  chain = {
    { type = "BindDialer", dial_addr = "unix://file1" },
    {
      type = "TLS",
      host = "www.1234.com",
      insecure = true

    },
    { type = "Trojan",     password = "mypassword" }
  },
  tag = "dial1"
}

local inbound_socks_http = {
  chain = {
    { type = "Listener",  listen_addr = "0.0.0.0:10800" },
    { type = "Socks5Http" }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_unix_trojan },
  inbounds = { inbound_socks_http }
}
