local outbound_direct = {
  { type = "Direct" },
}

local quic_config = {
  type = "Quic",
  key = "test2.key",
  cert = "test2.crt",
  listen_addr = "0.0.0.0:10801",
  alpn = { "h3" }
}

local inbound_quic_trojan = {
  quic_config,
  { type = "Trojan", password = "mypassword" }
}

Config = {
  outbounds = { dial1 = outbound_direct },
  inbounds = { listen1 = inbound_quic_trojan }
}
