local outbound_trojan = {
  chain = {
    { BindDialer = { dial_addr = "tcp://127.0.0.1:10801" } },
    {
      TLS = {
        host = "www.1234.com",
        insecure = true
      }
    },
    { Trojan = { password = "mypassword" } }
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

local inbound_dns_proxy = {
  chain = { {
    Listener = {
      listen_addr = "udp://0.0.0.0:20800",
      ext = { fixed_target_addr = "udp://8.8.8.8:53" }
    }
  } },
  tag = "listen2"
}

Config = {
  outbounds = { outbound_trojan },
  inbounds = { inbound_socks_http, inbound_dns_proxy }
}
