local outbound_direct = {
  chain = { { Direct = {} } },
  tag = "dial1"
}

local tls_config = {
  key = "test2.key",
  cert = "test2.crt",
  alpn = { "h2", "http/1.1" }
}

local lua_config = {
  handshake_function = "Handshake2",
  file_name = "lua_protocol_e1.lua"
}

local inbound_lua_trojan = {
  chain = {
    { Listener = { listen_addr = "0.0.0.0:10801" } },
    { TLS = tls_config },
    { Trojan = { password = "mypassword" } },
    { Lua = lua_config }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_direct },
  inbounds = { inbound_lua_trojan }
}
