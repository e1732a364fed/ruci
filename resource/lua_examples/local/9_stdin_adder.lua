local outbound_stdio = {
  chain = {
    { Stdio = {} }
  },
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
  outbounds = { outbound_stdio },
  inbounds = { inbound_stdio_adder }
}
