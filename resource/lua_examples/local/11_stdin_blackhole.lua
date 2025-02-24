local outbound_blackhole = {
  chain = { { type = "Blackhole" } },
  tag = "dial1"
}

local inbound_stdio_adder = {
  chain = {
    {
      type = "Stdio",
      ext = { pre_defined_early_data = "abc" }

    },
    { type = "Adder", value = 1 }
  },
  tag = "listen1"
}

Config = {
  inbounds = { inbound_stdio_adder },
  outbounds = { outbound_blackhole }

}
