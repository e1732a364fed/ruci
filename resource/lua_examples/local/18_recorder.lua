local recorder_config_direct = {
  type = "Recorder",
  output_file_extension = "cbor",
  label = "direct",
}

local recorder_config_socks5 = {
  type = "Recorder",
  output_file_extension = "cbor",
  label = "socks5",
}

local outbound_direct_recorder = {
  chain = {
    { type = "Direct" },
    recorder_config_direct
  },
  tag = "dial1"
}

local inbound_socks_recorder = {
  chain = {
    { type = "Listener",  listen_addr = "0.0.0.0:10800" },
    recorder_config_socks5,
    { type = "Socks5Http" }
  },
  tag = "listen1"
}

Config = {
  outbounds = { outbound_direct_recorder },
  inbounds = { inbound_socks_recorder }
}
