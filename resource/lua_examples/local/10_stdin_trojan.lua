local outbound_trojan = {
  { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
  {
    type = "TLS", host = "www.1234.com", insecure = true
  },
  { type = "Trojan",     password = "mypassword" }
}

local inbound_stdio_adder = {
  {
    type = "Stdio",
    ext = { pre_defined_early_data = "abc" }

  },
  { type = "Adder", value = 1 }
}

Config = {
  outbounds = { dial1 = outbound_trojan },
  inbounds = { listen1 = inbound_stdio_adder }
}
