local outbound_trojan = {
  { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
  {
    type = "TLS", host = "www.1234.com", insecure = true
  },
  { type = "Trojan",     password = "mypassword" }
}

local fileio_config = {
  type = "Fileio",
  i = "test.crt",
  o = "testfile.txt",
  sleep_interval = 500,
  bytes_per_turn = 10,
  ext = { fixed_target_addr = "fake.com:80" }
}

Config = {
  outbounds = { dial1 = outbound_trojan },
  inbounds = { listen1 = { fileio_config } }
}
