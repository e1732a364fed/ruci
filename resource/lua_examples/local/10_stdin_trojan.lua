local outbound_trojan = {
  chain = {
    { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
    {
      type = "TLS", host = "www.1234.com", insecure = true
    },
    { type = "Trojan",     password = "mypassword" }
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
  outbounds = { outbound_trojan },
  inbounds = { inbound_stdio_adder }
}
