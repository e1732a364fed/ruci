local outbound_socks5 = {
  chain = {
    { BindDialer = { dial_addr = "tcp://127.0.0.1:10801" } },
    { Socks5 = {} }
  },
  tag = "d1"
}

local stdio_config = {
  ext = {
    pre_defined_early_data = "abc",
    fixed_target_addr = "udp://127.0.0.1:20800"
  }
}

local inbound_stdio_adder = {
  chain = {
    { Stdio = stdio_config },
    { Adder = 1 }
  },
  tag = "in_stdio_adder_chain"
}

Config = {
  outbounds = { outbound_socks5 },
  inbounds = { inbound_stdio_adder }
}
