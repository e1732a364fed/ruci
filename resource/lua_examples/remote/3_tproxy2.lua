Config = {
  ["outbounds"] = {
    {
      ["chain"] = {
        {
          ["OptDirect"] = {
            ["sockopt"] = {
              ["bind_to_device"] = "wlp3s0",
              ["so_mark"] = 255
            },
            ["more_num_of_files"] = true,
          }
        }
      },
      ["tag"] = "dial1"
    }
  },
  ["fallback_route"] = {
    {
      "listen1",
      "fallback_d"
    }
  },
  ["inbounds"] = {
    {
      ["chain"] = {
        {
          ["Listener"] = {
            ["listen_addr"] = "0.0.0.0:10801",
          }
        },
        {
          ["TLS"] = {
            ["key"] = "test2.key",
            ["cert"] = "test2.crt",
            ["alpn"] = {
              "h2",
              "http/1.1"
            }
          }
        },
        {
          ["Trojan"] = {
            ["password"] = "mypassword",
          }
        }
      },
      ["tag"] = "listen1"
    }
  }
}