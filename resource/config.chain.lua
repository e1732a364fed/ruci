print("this is a lua config file")

-- lua 的好处有很多，你可以定义很多变量

listen = { Listener = { TcpListener = "0.0.0.0:10800" }  }
listen_chain1 = { listen, { Socks5 = {} }, }
listen_http = { listen, { Http = {} }, }

config = {
    listen = { {chain = listen_chain1, tag = "listen1"} },
    dial = { { tag="dial1", chain = { "Direct" } } }
}
