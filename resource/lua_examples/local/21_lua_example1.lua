local lua_config = {
  type = "Lua",
  handshake_function = "Handshake2",
  file_name = "lua_protocol_e1.lua"
}

local outbound_lua_trojan = {
  chain = {
    { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
    {
      type = "TLS", host = "www.1234.com", insecure = true
    },
    { type = "Trojan",     password = "mypassword" },
    lua_config
  },
  tag = "dial1"
}

local inbound_socks_http = {
  chain = {
    { type = "Listener",  listen_addr = "0.0.0.0:10800" },
    { type = "Socks5Http" }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_lua_trojan },
  inbounds = { inbound_socks_http }
}
