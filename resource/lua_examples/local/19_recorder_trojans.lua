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
      ["tag"] = "dial_direct"
    },
    {
      ["chain"] = {
        {
          ["BindDialer"] = {
            ["dial_addr"] = "tcp://127.0.0.1:10801"
          }
        },
        {
          ["NativeTLS"] = {
            ["host"] = "www.bilibili.com",
            ["insecure"] = true,
            ["alpn"] = {
              "http/1.1"
            }
          }
        },
        {
          ["Recorder"] = {
            ["serialize_format"] = "cbor",
            ["label"] = "trojan",
            ["no_truncate"] = true,
          }
        },
        {
          ["Trojan"] = "mypassword"
        }
      },
      ["tag"] = "dial_trojans"
    }
  },
  ["tag_route"] = {
    {
      "listen_socks5",
      "dial_trojans"
    },
    {
      "listen_trojans",
      "dial_direct"
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
      ["tag"] = "listen_socks5"
    },
    {
      ["chain"] = {
        {
          ["Listener"] = {
            ["listen_addr"] = "0.0.0.0:10801",
          }
        },
        {
          ["Recorder"] = {
            ["serialize_format"] = "cbor",
            ["label"] = "trojans",
            ["no_truncate"] = true,
          }
        },
        {
          ["TLS"] = {
            ["key"] = "test.key",
            ["cert"] = "test.crt",
            ["alpn"] = {
              "h2",
              "http/1.1"
            }
          }
        },
        {
          ["Trojan"] = {
            ["password"] = "mypassword",
          }
        }
      },
      ["tag"] = "listen_trojans"
    }
  }
}