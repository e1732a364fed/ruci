local quic_config = {
  type = "Quic",
  server_addr = "127.0.0.1:10801",
  alpn = { "h3" },
  cert = "test2.crt",
  server_name = "www.mytest.com"
}

local outbound_quic_trojan = {
  quic_config,
  { type = "Trojan", password = "mypassword" }
}

local inbound_socks_http = {
  { type = "Listener",  listen_addr = "0.0.0.0:10800" },
  { type = "Socks5Http" }
}

Config = {
  outbounds = { dial1 = outbound_quic_trojan },
  inbounds = { listen1 = inbound_socks_http }
}
