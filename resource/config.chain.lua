print("this is a lua config file")

-- lua 的好处有很多，你可以定义很多变量

listen = { Listener = { TcpListener = "0.0.0.0:10800" }  }
listen_socks5 = { listen, { Socks5 = {} }, }
listen_http = { listen, { Http = {} }, }
listen_socks5http = { listen, { Socks5Http = {} }, }

config = {
    listen = { {chain = listen_socks5http, tag = "listen1"} },
    dial = { { tag="dial1", chain = { "Direct" } } }
}
