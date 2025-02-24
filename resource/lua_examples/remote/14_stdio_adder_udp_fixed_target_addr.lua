local outbound_direct = {
  chain = { { type = "Direct" } },
  tag = "dial1"
}

local inbound_socks_http = {
  chain = {
    { type = "Listener",  listen_addr = "0.0.0.0:10801" },
    { type = "Socks5Http" }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_direct },
  inbounds = { inbound_socks_http }
}
