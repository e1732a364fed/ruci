local outbound_direct = {
  chain = { { Direct = {} } },
  tag = "dial1"
}

local quic_config = {
  key_path = "test2.key",
  cert_path = "test2.crt",
  listen_addr = "0.0.0.0:10801",
  alpn = { "h3" }
}

local inbound_quic_trojan = {
  chain = {
    { Quic = quic_config },
    { Trojan = { password = "mypassword" } }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_direct },
  inbounds = { inbound_quic_trojan }
}
