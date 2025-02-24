local outbound_socks5 = {
  chain = {
    { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
    { type = "Socks5" }
  },
  tag = "d1"
}

local stdio_config = {
  type = "Stdio",
  ext = {
    pre_defined_early_data = "abc",
    fixed_target_addr = "udp://127.0.0.1:20800"
  }
}

local inbound_stdio_adder = {
  chain = {
    stdio_config,
    { type = "Adder", value = 1 }
  },
  tag = "in_stdio_adder_chain"
}

Config = {
  outbounds = { outbound_socks5 },
  inbounds = { inbound_stdio_adder }
}
