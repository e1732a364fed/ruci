Config = {
  ["outbounds"] = {
    {
      ["chain"] = {
        {
          ["BindDialer"] = {
            ["dial_addr"] = "tcp://127.0.0.1:10801"
          }
        },
        {
          ["TLS"] = {
            ["host"] = "www.1234.com",
            ["insecure"] = true,
          }
        },
        {
          ["Trojan"] = "mypassword"
        }
      },
      ["tag"] = "dial1"
    }
  },
  ["inbounds"] = {
    {
      ["chain"] = {
        {
          ["Fileio"] = {
            ["i"] = "test.crt",
            ["sleep_interval"] = 500,
            ["bytes_per_turn"] = 10,
            ["o"] = "testfile.txt",
            ["ext"] = {
              ["fixed_target_addr"] = "fake.com:80"
            }
          }
        }
      },
      ["tag"] = "listen1"
    }
  }
}