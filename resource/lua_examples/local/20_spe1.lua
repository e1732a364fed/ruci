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
          ["Listener"] = {
            ["listen_addr"] = "0.0.0.0:10800",
          }
        },
        {
          ["Socks5Http"] = {
          }
        }
      },
      ["tag"] = "listen1"
    }
  }
}