local recorder_config_direct = {
  serialize_format = "cbor",
  label = "direct",
  no_truncate = true
}

local recorder_config_socks5 = {
  serialize_format = "cbor",
  label = "socks5",
  no_truncate = true
}

local outbound_direct_recorder = {
  chain = {
    { Direct = {} },
    { Recorder = recorder_config_direct }
  },
  tag = "dial1"
}

local inbound_socks_recorder = {
  chain = {
    { Listener = { listen_addr = "0.0.0.0:10800" } },
    { Recorder = recorder_config_socks5 },
    { Socks5Http = {} }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_direct_recorder },
  inbounds = { inbound_socks_recorder }
}
