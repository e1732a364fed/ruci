local outbound = {
  dial1 = { { type = "Direct" } },
}

local inbound = {
  listen1 = {
    { type = "Listener",  listen_addr = "0.0.0.0:10800" },
    { type = "Socks5Http" }
  },
}

Config = {
  outbounds = outbound,
  inbounds = inbound
}
