local outbound_stdio = {
  { type = "Stdio" }
}

local inbound_stdio_adder = {
  {
    type = "Stdio",
    ext = { pre_defined_early_data = "abc" }

  },
  { type = "Adder", value = 1 }
}

Config = {
  outbounds = { dial1 = outbound_stdio },
  inbounds = { listen1 = inbound_stdio_adder }
}
