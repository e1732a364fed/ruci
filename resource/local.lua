print("this is a lua config file")

-- lua 的好处有很多，你可以定义很多变量

listen = { Listener =  "0.0.0.0:10800"   }
l2 = { Listener =   "0.0.0.0:20800"   }
l3 = { Listener =   "0.0.0.0:30800"   }

listen_socks5 = { listen, { Socks5 = {} }, }
listen_http = { listen, { Http = {} }, }
listen_socks5http = { listen, { Socks5Http = {} }, }

tlsout = { TLS = { host = "www.1234.com", insecure = true } }

tlsin = { TLS = {  cert = "test.crt", key = "test.key" } }


listen_trojan = { listen, { Trojan = { password = "mypassword" } }, }

dial = { Dialer =  "tcp://0.0.0.0:10801" }

dial_trojan_chain = { dial,tlsout,  { Trojan = "mypassword"} }

stdio_socks5_chain = { { Stdio={ fixed_target_addr= "fake.com:80" } } , { Socks5 = {} } }

-- stdin + 1 , 在命令行输入 a, 会得到b，输入1，得2，依此类推
in_stdio_adder_chain = { { Stdio={ fixed_target_addr= "fake.com:80", pre_defined_early_data = "abc" } } , { Adder = 1 } } 
--这里 用fake.com 的目的是, 保证我们的输入有一个目标. 这是代理所要求的.

out_stdio_chain = { { Stdio={} } }

direct_out_chain = { "Direct" }

---[=[

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
-- default counterpart for remote.lua

config = {
    inbounds = { {chain = listen_socks5http, tag = "listen1"} },
    outbounds = { { tag="dial1", chain = dial_trojan_chain } }

--[[
这个 config 块是演示 inbound 是 socks5http, outbound 是 trojan+若干tls 的情况

它是一个基本的远程代理示例. 运行它, 设置您的系统代理为相应端口,
并参照 remote.lua 在另一个终端 运行 另一部分,
看看能不能正常访问网络吧
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


--[[


config = {

-- stdin + 1 -> trojan_out

    inbounds = { 
        {chain = in_stdio_adder_chain, tag = "listen1"} ,
    },

    outbounds = { { tag="dial1", chain = dial_trojan_chain } }
}

--]]


--[[


config = {

-- stdin + 1 -> blackhole

    inbounds = { 
        {chain = in_stdio_adder_chain, tag = "listen1"} ,
    },

    --expected warn: dial out client stream got consumed

    outbounds = { { tag="dial1", chain = { "Blackhole" } } }
}

--]]



--[=[

config = {
    inbounds = { 
        {chain = listen_socks5http, tag = "l1"}, {chain = {l2,tlsin}, tag = "l2"} , 
        {chain = {l3,tlsin}, tag = "l3"} 
    } ,
    outbounds = { 
        { tag="d1", chain = { "Direct" } } ,  { tag="d2", chain = { dial, tlsout } } 
    },

    tag_route = {  { "l1","d1" },{ "l2","d2"},{"l3","d2"} }

--[[
这个 config 块是演示 多in多out的情况, 只要outbounds有多个，您就应该考虑使用路由配置
该 tag_route 示例明确指出，l1将被路由到d1, l2 -> d2, l3 -> d2
--]]

}

--]=]

