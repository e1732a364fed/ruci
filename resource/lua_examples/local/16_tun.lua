Config = {
  ["outbounds"] = {
    {
      ["chain"] = {
        {
          ["OptDialer"] = {
            ["sockopt"] = {
              ["bind_to_device"] = "en0",
            },
            ["dial_addr"] = "tcp://192.168.0.204:10801"
          }
        },
        {
          ["TLS"] = {
            ["host"] = "www.1234.com",
            ["insecure"] = true,
          }
        },
        {
          ["WebSocket"] = {
            ["path"] = "/path1",
            ["use_early_data"] = true,
            ["authority"] = "myhost"
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
          ["BindDialer"] = {
            ["in_auto_route"] = {
              ["tun_dev_name"] = "utun321",
              ["dns_list"] = {
                "114.114.114.114"
              },
              ["original_dev_name"] = "enp0s1",
              ["tun_gateway"] = "10.0.0.1",
              ["router_ip"] = "192.168.0.1"
            },
            ["bind_addr"] = "ip://10.0.0.1:24#utun321",
          }
        }
      },
      ["tag"] = "listen1"
    }
  }
}