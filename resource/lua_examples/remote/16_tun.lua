Config = {
  ["outbounds"] = {
    {
      ["chain"] = {
        {
          ["BindDialer"] = {
            ["bind_addr"] = "ip://10.0.0.2:24#utun321",
            ["out_auto_route"] = {
              ["tun_dev_name"] = "utun321",
              ["original_dev_name"] = "enp0s1",
              ["router_ip"] = "192.168.0.1"
            },
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
          ["HttpFilter"] = {
            ["path"] = "/path1",
            ["authority"] = "myhost"
          }
        },
        {
          ["WebSocket"] = {
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