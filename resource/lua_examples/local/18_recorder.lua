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
  { type = "Direct" },
  recorder_config_direct
}

local inbound_socks_recorder = {
  { type = "Listener",  listen_addr = "0.0.0.0:10800" },
  recorder_config_socks5,
  { type = "Socks5Http" }
}

Config = {
  outbounds = { dial1 = outbound_direct_recorder },
  inbounds = { listen1 = inbound_socks_recorder }
}
