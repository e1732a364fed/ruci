print("this is a lua config file")

-- lua 的好处有很多，你可以定义很多变量

listen = { Listener = { TcpListener = "0.0.0.0:10801" }  }
listen_socks5 = { listen, { Socks5 = {} }, }
listen_http = { listen, { Http = {} }, }
listen_socks5http = { listen, { Socks5Http = {} }, }

listen_trojan = { listen, { Trojan = { password = "mypassword" } }, }

dial = { Dialer = { TcpDialer = "0.0.0.0:10801" }}

dial_trojan = { dial, { Trojan = "mypassword"} }

-- config = {
--     listen = { {chain = listen_socks5http, tag = "listen1"} },
--     dial = { { tag="dial1", chain = { "Direct" } } }
-- }

config = {
    listen = { 
        {chain = listen_trojan, tag = "listen1"} ,
    },

    dial = { { tag="dial1", chain =  { "Direct" } } }
}

