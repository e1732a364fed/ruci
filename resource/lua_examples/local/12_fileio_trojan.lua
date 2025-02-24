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

local fileio_config = {
  type = "Fileio",
  i = "test.crt",
  o = "testfile.txt",
  sleep_interval = 500,
  bytes_per_turn = 10,
  ext = { fixed_target_addr = "fake.com:80" }
}

local inbound_fileio = {
  chain = {
    fileio_config
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_trojan },
  inbounds = { inbound_fileio }
}
