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

stdio_socks5_chain = { { Stdio="fake.com:80" } , { Socks5 = {} } }

-- stdin + 1 , 在命令行输入 a, 会得到b，输入1，得2，依此类推
in_stdio_adder_chain = { { Stdio="fake.com:80" } , { Adder = 1 } } 
--这里 用fake.com 的目的是, 保证我们的输入有一个目标. 这是代理所要求的.

out_stdio_chain = { { Stdio="" } }

direct_out_chain = { "Direct" }

--[=[

config = {
    inbounds = { {chain = listen_socks5http, tag = "listen1"} },
    outbounds = { { tag="dial1", chain = { "Direct" } } }

--[[
这个 config 块是演示 inbound 是 socks5http, outbound 是 direct 的情况

它是一个基本的本地代理示例. 运行它, 设置您的系统代理为相应端口, 看看能不能正常访问网络吧
--]]

}

--]=]


--[=[
config = {

    inbounds = { 
        {chain = in_stdio_adder_chain, tag = "listen1"} ,
    },

--[[
这个 config 块是演示 inbound 是 stdio (命令行)+1, outbound 也是stdio的情况, 

此时需要注意, 该配置下 命令行 的输入会既用作 inbound 的输入, 也用作 outbound 的输入;

在实际操作中, 您会看到, 输入被in和out轮流使用, 因此会有 一次+1, 一次不+1的情况轮流出现

--]]

    outbounds = { { tag="dial1", chain = out_stdio_chain } }
}

--]=]



config = {

    inbounds = { 
        {chain = in_stdio_adder_chain, tag = "listen1"} ,
    },

    outbounds = { { tag="dial1", chain = dial_trojan_chain } }
}

