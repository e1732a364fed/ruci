local spe1_config = {
  type = "SPE1",
  qa = {
    { "q1", "a1" },
    { "q2", "a2" }
  }
}

local outbound_spe1_trojan = {
  { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
  spe1_config,
  { type = "Trojan",     password = "mypassword" }
}

local inbound_socks_http = {
  { type = "Listener",  listen_addr = "0.0.0.0:10800" },
  { type = "Socks5Http" }
}

Config = {
  outbounds = { dial1 = outbound_spe1_trojan },
  inbounds = { listen1 = inbound_socks_http }
}
