local outbound_blackhole = {
  chain = { "Blackhole" },
  tag = "dial1"
}

local inbound_stdio_adder = {
  chain = {
    {
      Stdio = {
        ext = { pre_defined_early_data = "abc" }
      }
    },
    { Adder = 1 }
  },
  tag = "listen1"
}

Config = {
  inbounds = { inbound_stdio_adder },
  outbounds = { outbound_blackhole }

}
