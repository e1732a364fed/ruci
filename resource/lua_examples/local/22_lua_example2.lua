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
            ["host"] = "www.1234.com",
            ["insecure"] = true,
          }
        },
        {
          ["Trojan"] = "mypassword"
        },
        {
          ["Lua"] = {
            ["handshake_function"] = "Handshake",
            ["file_name"] = "lua_protocol_e2_mathadd.lua"
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
    }
  }
}