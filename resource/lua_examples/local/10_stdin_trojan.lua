local outbound_trojan = {
  chain = {
    { BindDialer = { dial_addr = "tcp://127.0.0.1:10801" } },
    {
      TLS = {
        host = "www.1234.com",
        insecure = true
      }
    },
    { Trojan = "mypassword" }
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
  outbounds = { outbound_trojan },
  inbounds = { inbound_stdio_adder }
}
