print("this is a lua config file")

-- lua 的好处有很多，你可以定义很多变量

tcp = { Listener = { TcpListener = "0.0.0.0:10801" }  }
socks5_chain = { tcp, { Socks5 = {} }, }
http_chain = { tcp, { Http = {} }, }
socks5http_chain = { tcp, { Socks5Http = {} }, }

tls = { TLS = {  cert = "test.crt", key = "test.key" } }

trojan_chain = { tcp, tls, { Trojan = { password = "mypassword" } }, }

dial = { Dialer = { TcpDialer = "0.0.0.0:10801" }}

dial_trojan = { dial, { Trojan = "mypassword"} }


stdio_out = { Stdio={}  }

direct_out_chain = { "Direct" }

-- config = {
--     inbounds = { {chain = listen_socks5http, tag = "listen1"} },
--     outbounds = { { tag="dial1", chain = { "Direct" } } }
-- }

config = {
    inbounds = { 
        {chain = trojan_chain, tag = "listen1"} ,
    },

    outbounds = { { tag="dial1", chain = direct_out_chain  } }
}

