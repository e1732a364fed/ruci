local recorder_config = {
  direct = {
    serialize_format = "cbor",
    label = "direct",
    no_truncate = true
  },
  trojan = {
    serialize_format = "cbor",
    label = "trojan",
    no_truncate = true
  },
  socks5 = {
    serialize_format = "cbor",
    label = "socks5",
    no_truncate = true
  },
  trojans = {
    serialize_format = "cbor",
    label = "trojans",
    no_truncate = true
  }
}

local outbound_direct = {
  chain = {
    { Direct = {} },
    { Recorder = recorder_config.direct }
  },
  tag = "dial_direct"
}

local outbound_trojan = {
  chain = {
    { BindDialer = { dial_addr = "tcp://127.0.0.1:10801" } },
    {
      NativeTLS = {
        host = "www.bilibili.com",
        insecure = true,
        alpn = { "http/1.1" }
      }
    },
    { Recorder = recorder_config.trojan },
    { Trojan = { password = "mypassword" } }
  },
  tag = "dial_trojans"
}

local inbound_socks5 = {
  chain = {
    { Listener = { listen_addr = "0.0.0.0:10800" } },
    { Recorder = recorder_config.socks5 },
    { Socks5Http = {} }
  },
  tag = "listen_socks5"
}

local inbound_trojan = {
  chain = {
    { Listener = { listen_addr = "0.0.0.0:10801" } },
    { Recorder = recorder_config.trojans },
    {
      TLS = {
        key = "test.key",
        cert = "test.crt",
        alpn = { "h2", "http/1.1" }
      }
    },
    { Trojan = { password = "mypassword" } }
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
