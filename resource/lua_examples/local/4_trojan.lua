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
    },
    {
      ["chain"] = {
        {
          ["Listener"] = {
            ["listen_addr"] = "udp://0.0.0.0:20800",
            ["ext"] = {
              ["fixed_target_addr"] = "udp://8.8.8.8:53"
            }
          }
        }
      },
      ["tag"] = "listen2"
    }
  }
}