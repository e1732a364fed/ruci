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
          ["Trojan"] = {
            ["password"] = "mypassword",
          }
        },
        {
          ["Lua"] = {
            ["handshake_function"] = "Handshake",
            ["file_name"] = "lua_protocol_e2_mathadd.lua"
          }
        }
      },
      ["tag"] = "listen1"
    }
  }
}