Config = {
  ["outbounds"] = {
    {
      ["chain"] = {
        {
          ["Direct"] = {
          }
        }
      },
      ["tag"] = "dial1"
    },
    {
      ["chain"] = {
        {
          ["BindDialer"] = {
            ["dial_addr"] = "tcp://0.0.0.0:80"
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