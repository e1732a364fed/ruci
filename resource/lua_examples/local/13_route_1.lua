local outbound_direct = {
  { type = "Direct" },
}

local outbound_trojan = {
  { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
  {
    type = "TLS", host = "www.1234.com", insecure = true
  },
  { type = "Trojan",     password = "mypassword" }
}

local outbound_fallback = {
  {
    type = "BindDialer", dial_addr = "tcp://127.0.0.1:80"
  },
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

local inbound_tls = {
  { type = "Listener", listen_addr = "[::1]:30800" },
  {
    type = "TLS",
    key = "test.key",
    cert = "test.crt"

  },
}

Config = {
  outbounds = { d1 = outbound_direct, d2 = outbound_trojan, fallback_d = outbound_fallback },
  inbounds = { l1 = inbound_socks_http, l2 = inbound_dns_proxy, l3 = inbound_tls },

  routes = {
    tag_route = {
      { "l1", "d1" },
      { "l2", "d2" },
      { "l3", "d2" }
    },
    fallback_route = { { "l1", "fallback_d" } }
  }

}
