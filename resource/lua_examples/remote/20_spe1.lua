local outbound_direct = {
  { type = "Direct" },
}

local outbound_fallback = {
  { type = "BindDialer", dial_addr = "tcp://0.0.0.0:80" }
}

local spe1_config = {
  type = "SPE1",
  qa = {
    { "q1", "a1" },
    { "q2", "a2" }
  }
}

local inbound_spe1_trojan = {
  { type = "Listener", listen_addr = "0.0.0.0:10801" },
  spe1_config,
  { type = "Trojan",   password = "mypassword" }
}

Config = {
  outbounds = { dial1 = outbound_direct, fallback_d = outbound_fallback },
  inbounds = { listen1 = inbound_spe1_trojan },
  routes = {
    fallback_route = { { "listen1", "fallback_d" } }

  }
}
