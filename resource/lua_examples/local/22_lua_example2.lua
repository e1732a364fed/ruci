local lua_config = {
  handshake_function = "Handshake",
  file_name = "lua_protocol_e2_mathadd.lua"
}

local outbound_lua_trojan = {
  chain = {
    { BindDialer = { dial_addr = "tcp://127.0.0.1:10801" } },
    {
      TLS = {
        host = "www.1234.com",
        insecure = true
      }
    },
    { Trojan = "mypassword" },
    { Lua = lua_config }
  },
  tag = "dial1"
}

local inbound_socks_http = {
  chain = {
    { Listener = { listen_addr = "0.0.0.0:10800" } },
    { Socks5Http = {} }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_lua_trojan },
  inbounds = { inbound_socks_http }
}
