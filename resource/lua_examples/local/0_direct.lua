local outbound = {
  chain = { { Direct = {} } },
  tag = "dial1"
}

local inbound = {
  chain = {
    { Listener = { listen_addr = "0.0.0.0:10800" } },
    { Socks5Http = {} }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound },
  inbounds = { inbound }
}
