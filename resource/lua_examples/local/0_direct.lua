local outbounds = {
  Direct = { { type = "Direct" } },
  Reject = {
    {
      type = "Blackhole"
    }
  }
}

local inbounds = {
  socks5_listen = {
    { type = "Listener",  listen_addr = "0.0.0.0:10800" },
    { type = "Socks5Http" }
  },

}

Config = {
  outbounds = outbounds,
  inbounds = inbounds,
  routes = {
    clash_rules = "test_clash_rules.yaml",
    geosite_gfw = {
      api_url = "http://127.0.0.1:5134/check",
      ok_ban_out_tag = { "Direct", "Reject" }
    }
  }
}
