local spe1_config = {
  qa = {
    { "q1", "a1" },
    { "q2", "a2" }
  }
}

local outbound_spe1_trojan = {
  chain = {
    { BindDialer = { dial_addr = "tcp://127.0.0.1:10801" } },
    { SPE1 = spe1_config },
    { Trojan = "mypassword" }
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

Config = {
  outbounds = { outbound_spe1_trojan },
  inbounds = { inbound_socks_http }
}
