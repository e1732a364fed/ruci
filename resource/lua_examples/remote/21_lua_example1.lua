local outbound_direct = {
  { type = "Direct" },
}

local tls_config = {
  type = "TLS",
  key = "test2.key",
  cert = "test2.crt",
  alpn = { "h2", "http/1.1" }
}

local lua_config = {
  type = "Lua",
  handshake_function = "Handshake2",
  file_name = "lua_protocol_e1.lua"
}

local inbound_lua_trojan = {
  { type = "Listener", listen_addr = "0.0.0.0:10801" },
  tls_config,
  { type = "Trojan",   password = "mypassword" },
  lua_config
}

Config = {
  outbounds = { dial1 = outbound_direct },
  inbounds = { listen1 = inbound_lua_trojan }
}
