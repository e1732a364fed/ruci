Config = {
  ["outbounds"] = {
    {
      ["chain"] = {
        {
          ["Direct"] = {
          }
        },
        {
          ["Recorder"] = {
            ["serialize_format"] = "cbor",
            ["label"] = "direct",
            ["no_truncate"] = true,
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
          ["Recorder"] = {
            ["serialize_format"] = "cbor",
            ["label"] = "socks5",
            ["no_truncate"] = true,
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