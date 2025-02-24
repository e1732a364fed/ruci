local outbound_stdio = {
  chain = {
    { type = "Stdio" }
  },
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
  outbounds = { outbound_stdio },
  inbounds = { inbound_stdio_adder }
}
