local outbound_direct = {
  chain = { { type = "Direct" } },
  tag = "dial1"
}

local tls_config = {
  type = "TLS",
  key = "test2.key",
  cert = "test2.crt",
  alpn = { "h2", "http/1.1" }
}

local lua_config = {
  type = "Lua",
  handshake_function = "Handshake",
  file_name = "lua_protocol_e2_mathadd.lua"
}

local inbound_lua_trojan = {
  chain = {
    { type = "Listener", listen_addr = "0.0.0.0:10801" },
    tls_config,
    { type = "Trojan",   password = "mypassword" },
    lua_config
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_direct },
  inbounds = { inbound_lua_trojan }
}
