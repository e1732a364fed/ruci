local outbound_blackhole = {
  { type = "Blackhole" },
}

local inbound_stdio_adder = {
  {
    type = "Stdio",
    ext = { pre_defined_early_data = "abc" }

  },
  { type = "Adder", value = 1 }
}

Config = {
  inbounds = { listen1 = inbound_stdio_adder },
  outbounds = { dial1 = outbound_blackhole }

}
