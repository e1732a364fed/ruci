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
            ["host"] = "www.google.com",
            ["insecure"] = true,
            ["alpn"] = {
              "h2",
              "http/1.1"
            }
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
        },
        {
          ["MITM"] = {
            ["key"] = "test_ca_key.pem",
            ["cert"] = "test_ca_cert.pem",
            ["alpn"] = {
              "h2",
              "http/1.1"
            }
          }
        }
      },
      ["tag"] = "listen1"
    }
  }
}