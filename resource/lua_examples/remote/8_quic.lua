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
    }
  },
  ["inbounds"] = {
    {
      ["chain"] = {
        {
          ["Quic"] = {
            ["key_path"] = "test2.key",
            ["alpn"] = {
              "h3"
            },
            ["listen_addr"] = "0.0.0.0:10801",
            ["cert_path"] = "test2.crt"
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