print("this is a lua config file")

-- lua 的好处有很多，你可以定义很多变量

tcp = { Listener =  "0.0.0.0:10801"   }
unix = { Listener =  "unix://file1"   }

socks5_chain = { tcp, { Socks5 = {} }, }
http_chain = { tcp, { Http = {} }, }
socks5http_chain = { tcp, { Socks5Http = {} }, }


tls = { TLS = {  cert = "test.crt", key = "test.key" } }

trojan_in = { Trojan = { password = "mypassword" } }

trojan_chain = { tcp, tls,  trojan_in, }

dial = { Dialer =  "tcp://0.0.0.0:10801" }

dial_trojan = { dial, { Trojan = "mypassword"} }


out_stdio_chain = { { Stdio={} } }

direct_out_chain = { "Direct" }




config = {
    inbounds = { 
        {chain = trojan_chain, tag = "listen1"} ,
        --{chain = socks5http_chain, tag = "listen1"} ,
        --{chain =  { unix,tls, trojan_in }, tag = "listen1"} ,

        --{chain = { { Dialer =  "udp://127.0.0.1:20800" } , "Echo" }, tag = "udp_echo"} ,
    },

    --outbounds = { { tag="dial1", chain = direct_out_chain  } }
    outbounds = { { tag="dial1", chain = out_stdio_chain  } } --以命令行为出口
}

