Config = {
  ["outbounds"] = {
    {
      ["chain"] = {
        {
          ["Direct"] = {
            ["leak_target_addr"] = true,
          }
        },
        {
          ["TLS"] = {
            ["insecure"] = false,
            ["alpn"] = {
              "h2",
              "http/1.1"
            }
          }
        }
      },
      ["tag"] = "dial1"
    },
    {
      ["chain"] = {
        {
          ["BindDialer"] = {
            ["dial_addr"] = "tcp://0.0.0.0:4433"
          }
        }
      },
      ["tag"] = "fallback_d"
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