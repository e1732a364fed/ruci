local outbound_unix_trojan = {
  { type = "BindDialer", dial_addr = "unix://file1" },
  {
    type = "TLS",
    host = "www.1234.com",
    insecure = true

  },
  { type = "Trojan",     password = "mypassword" }
}

local inbound_socks_http = {
  { type = "Listener",  listen_addr = "0.0.0.0:10800" },
  { type = "Socks5Http" }
}

Config = {
  outbounds = { dial1 = outbound_unix_trojan },
  inbounds = { listen1 = inbound_socks_http }
}
