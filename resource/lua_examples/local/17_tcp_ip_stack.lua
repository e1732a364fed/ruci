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
              ["original_dev_name"] = "en0",
              ["tun_gateway"] = "10.0.0.1",
              ["router_ip"] = "192.168.0.1"
            },
            ["bind_addr"] = "ip://10.0.0.1:24#utun321",
          }
        },
        "Stack"
      },
      ["tag"] = "listen1"
    }
  }
}