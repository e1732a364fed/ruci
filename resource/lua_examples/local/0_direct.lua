local outbound = {
  chain = { { type = "Direct" } },
  tag = "dial1"
}

local inbound = {
  chain = {
    { type = "Listener",  listen_addr = "0.0.0.0:10800" },
    { type = "Socks5Http" }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound },
  inbounds = { inbound }
}
