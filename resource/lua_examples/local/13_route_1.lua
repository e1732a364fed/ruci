local outbound_direct = {
  chain = { { type = "Direct" } },
  tag = "d1"
}

local outbound_trojan = {
  chain = {
    { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
    {
      type = "TLS", host = "www.1234.com", insecure = true
    },
    { type = "Trojan",     password = "mypassword" }
  },
  tag = "d2"
}

local outbound_fallback = {
  chain = { {
    type = "BindDialer", dial_addr = "tcp://127.0.0.1:80"
  } },
  tag = "fallback_d"
}

local inbound_socks_http = {
  chain = {
    { type = "Listener",  listen_addr = "0.0.0.0:10800" },
    { type = "Socks5Http" }
  },
  tag = "l1"
}

local inbound_dns_proxy = {
  chain = { {
    type = "Listener",
    listen_addr = "udp://0.0.0.0:20800",
    ext = { fixed_target_addr = "udp://8.8.8.8:53" }

  } },
  tag = "l2"
}

local inbound_tls = {
  chain = {
    { type = "Listener", listen_addr = "[::1]:30800" },
    {
      type = "TLS",
      key = "test.key",
      cert = "test.crt"

    }
  },
  tag = "l3"
}

Config = {
  outbounds = { outbound_direct, outbound_trojan, outbound_fallback },
  inbounds = { inbound_socks_http, inbound_dns_proxy, inbound_tls },
  tag_route = {
    { "l1", "d1" },
    { "l2", "d2" },
    { "l3", "d2" }
  },
  fallback_route = { { "l1", "fallback_d" } }
}
