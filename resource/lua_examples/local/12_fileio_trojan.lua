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

local fileio_config = {
  i = "test.crt",
  o = "testfile.txt",
  sleep_interval = 500,
  bytes_per_turn = 10,
  ext = { fixed_target_addr = "fake.com:80" }
}

local inbound_fileio = {
  chain = {
    { Fileio = fileio_config }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_trojan },
  inbounds = { inbound_fileio }
}
