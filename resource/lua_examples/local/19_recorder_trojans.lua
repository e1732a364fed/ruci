local recorder_config = {
  direct = {
    type = "Recorder",
    output_file_extension = "cbor",
    label = "direct",
  },
  trojan = {
    type = "Recorder",
    output_file_extension = "cbor",
    label = "trojan",
  },
  socks5 = {
    type = "Recorder",
    output_file_extension = "cbor",
    label = "socks5",
  },
  trojans = {
    type = "Recorder",
    output_file_extension = "cbor",
    label = "trojans",
  }
}

local outbound_direct = {
  chain = {
    { type = "Direct" },
    recorder_config.direct
  },
  tag = "dial_direct"
}

local outbound_trojan = {
  chain = {
    { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
    {
      type = "NativeTLS",
      host = "www.bilibili.com",
      insecure = true,
      alpn = { "http/1.1" }

    },
    recorder_config.trojan,
    { type = "Trojan",     password = "mypassword" }
  },
  tag = "dial_trojans"
}

local inbound_socks5 = {
  chain = {
    { type = "Listener",  listen_addr = "0.0.0.0:10800" },
    recorder_config.socks5,
    { type = "Socks5Http" }
  },
  tag = "listen_socks5"
}

local inbound_trojan = {
  chain = {
    { type = "Listener", listen_addr = "0.0.0.0:10801" },
    recorder_config.trojans,
    {
      type = "TLS",
      key = "test.key",
      cert = "test.crt",
      alpn = { "h2", "http/1.1" }

    },
    { type = "Trojan",   password = "mypassword" }
  },
  tag = "listen_trojans"
}

Config = {
  outbounds = { outbound_direct, outbound_trojan },
  inbounds = { inbound_socks5, inbound_trojan },
  tag_route = {
    { "listen_socks5",  "dial_trojans" },
    { "listen_trojans", "dial_direct" }
  }
}
