local lua_config = {
  type = "Lua",
  handshake_function = "Handshake",
  file_name = "lua_protocol_e2_mathadd.lua"
}

local outbound_lua_trojan = {
  { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
  {
    type = "TLS", host = "www.1234.com", insecure = true
  },
  { type = "Trojan",     password = "mypassword" },
  lua_config
}

local inbound_socks_http = {
  { type = "Listener",  listen_addr = "0.0.0.0:10800" },
  { type = "Socks5Http" }
}

Config = {
  outbounds = { dial1 = outbound_lua_trojan },
  inbounds = { listen1 = inbound_socks_http }
}
