print("this is a lua config file")

-- lua 的好处有很多，你可以定义很多变量

listen = { Listener = { TcpListener = "0.0.0.0:10800" }  }
listen_socks5 = { listen, { Socks5 = {} }, }
listen_http = { listen, { Http = {} }, }
listen_socks5http = { listen, { Socks5Http = {} }, }

tls = { TLS = { host = "www.1234.com", insecure = true } }

listen_trojan = { listen, { Trojan = { password = "mypassword" } }, }

dial = { Dialer = { TcpDialer = "0.0.0.0:10801" }}

dial_trojan_chain = { dial,tls, { Trojan = "mypassword"} }

-- config = {
--     listen = { {chain = listen_socks5http, tag = "listen1"} },
--     dial = { { tag="dial1", chain = { "Direct" } } }
-- }

config = {
    listen = { 
        {chain = listen_socks5, tag = "listen1"} ,
    },

    dial = { { tag="dial1", chain = dial_trojan_chain } }
}

