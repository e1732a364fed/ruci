local quic_config = {
  type = "Quic",
  server_addr = "127.0.0.1:10801",
  alpn = { "h3" },
  cert = "test2.crt",
  server_name = "www.mytest.com"
}

local outbound_quic_trojan = {
  chain = {
    quic_config,
    { type = "Trojan", password = "mypassword" }
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
  outbounds = { outbound_quic_trojan },
  inbounds = { inbound_socks_http }
}
