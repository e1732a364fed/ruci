local outbound_direct = {
  { type = "Direct" },
}

local inbound_socks_http = {
  { type = "Listener",  listen_addr = "0.0.0.0:10801" },
  { type = "Socks5Http" }
}

Config = {
  outbounds = { dial1 = outbound_direct },
  inbounds = { listen1 = inbound_socks_http }
}
