Config = {
  ["outbounds"] = {
    {
      ["chain"] = {
        {
          ["Quic"] = {
            ["server_addr"] = "127.0.0.1:10801",
            ["alpn"] = {
              "h3"
            },
            ["cert_path"] = "test2.crt",
            ["server_name"] = "www.mytest.com"
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