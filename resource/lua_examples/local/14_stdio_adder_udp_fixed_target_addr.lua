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
          ["Socks5"] = {
          }
        }
      },
      ["tag"] = "d1"
    }
  },
  ["inbounds"] = {
    {
      ["chain"] = {
        {
          ["Stdio"] = {
            ["ext"] = {
              ["pre_defined_early_data"] = "abc",
              ["fixed_target_addr"] = "udp://127.0.0.1:20800"
            },
          }
        },
        {
          ["Adder"] = 1
        }
      },
      ["tag"] = "in_stdio_adder_chain"
    }
  }
}