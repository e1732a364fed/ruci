local outbound_direct = {
  { type = "Direct" },
}

local inbound_unix_tls_trojan = {
  { type = "Listener", listen_addr = "unix://file1" },
  {
    type = "TLS",
    key = "test2.key",
    cert = "test2.crt",
    alpn = { "h2", "http/1.1" }
  },
  { type = "Trojan",   password = "mypassword" }
}

Config = {
  outbounds = { dial1 = outbound_direct },
  inbounds = { listen1 = inbound_unix_tls_trojan }
}
