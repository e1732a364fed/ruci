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
          ["SPE1"] = {
            ["qa"] = {
              {
                "q1",
                "a1"
              },
              {
                "q2",
                "a2"
              }
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