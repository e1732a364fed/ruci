Config = {
  ["outbounds"] = {
    {
      ["chain"] = {
        {
          ["Direct"] = {
            ["dns_client"] = {
              ["static_pairs"] = {
                ["www.baidu.com"] = "103.235.47.188"
              },
              ["ip_strategy"] = "Ipv4Only",
              ["dns_server_list"] = {
                {
                  "127.0.0.1:20800",
                  "udp"
                }
              }
            }
          }
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