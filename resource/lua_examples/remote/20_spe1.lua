local outbound_direct = {
  chain = { { type = "Direct" } },
  tag = "dial1"
}

local outbound_fallback = {
  chain = { {
    type = "BindDialer", dial_addr = "tcp://0.0.0.0:80" }
  },
  tag = "fallback_d"
}

local spe1_config = {
  type = "SPE1",
  qa = {
    { "q1", "a1" },
    { "q2", "a2" }
  }
}

local inbound_spe1_trojan = {
  chain = {
    { type = "Listener", listen_addr = "0.0.0.0:10801" },
    spe1_config,
    { type = "Trojan",   password = "mypassword" }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_direct, outbound_fallback },
  inbounds = { inbound_spe1_trojan },
  fallback_route = { { "listen1", "fallback_d" } }
}
