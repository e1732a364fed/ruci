local outbound_trojan = {
  { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
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

local inbound_dns_proxy = {
  {
    type = "Listener",
    listen_addr = "udp://0.0.0.0:20800",
    ext = { fixed_target_addr = "udp://8.8.8.8:53" }

  },
}

Config = {
  outbounds = { dial1 = outbound_trojan },
  inbounds = { listen1 = inbound_socks_http, listen2 = inbound_dns_proxy }
}
