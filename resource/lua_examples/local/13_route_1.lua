Config = {
  ["outbounds"] = {
    {
      ["chain"] = {
        {
          ["Direct"] = {
          }
        }
      },
      ["tag"] = "d1"
    },
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
      ["tag"] = "d2"
    },
    {
      ["chain"] = {
        {
          ["BindDialer"] = {
            ["dial_addr"] = "tcp://127.0.0.1:80"
          }
        }
      },
      ["tag"] = "fallback_d"
    }
  },
  ["tag_route"] = {
    {
      "l1",
      "d1"
    },
    {
      "l2",
      "d2"
    },
    {
      "l3",
      "d2"
    }
  },
  ["fallback_route"] = {
    {
      "l1",
      "fallback_d"
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
      ["tag"] = "l1"
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
      ["tag"] = "l2"
    },
    {
      ["chain"] = {
        {
          ["Listener"] = {
            ["listen_addr"] = "[::1]:30800",
          }
        },
        {
          ["TLS"] = {
            ["key"] = "test.key",
            ["cert"] = "test.crt",
          }
        }
      },
      ["tag"] = "l3"
    }
  }
}