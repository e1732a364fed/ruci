local outbound_socks5 = {
  { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
  { type = "Socks5" }
}

local stdio_config = {
  type = "Stdio",
  ext = {
    pre_defined_early_data = "abc",
    fixed_target_addr = "udp://127.0.0.1:20800"
  }
}

local inbound_stdio_adder = {
  stdio_config,
  { type = "Adder", value = 1 }
}

Config = {
  outbounds = { d1 = outbound_socks5 },
  inbounds = { in_stdio_adder_chain = inbound_stdio_adder }
}
