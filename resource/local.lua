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

trojan_in = { Trojan = { password = "mypassword" } }

listen_trojan = { listen, trojan_in, }

dial = { Dialer =  "tcp://0.0.0.0:10801" }

trojan_out =  { Trojan = "mypassword"}

dial_trojan_chain = { dial,tlsout, trojan_out }

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
    inbounds = { {chain = listen_socks5http, tag = "listen1"} },
    outbounds = { 
        { 
            tag="dial1", 
            chain = { { Dialer =  "unix://file1" }, tlsout, trojan_out } 
        } 
    }

--[[
这个 config 块 与上面的 示例类似, 但是它 的 dial 是用的 unix domain socket 
与此对应的 remote.lua 中 也应该是 unix 的监听

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


--[[


config = {

-- fileio -> trojan_out

    inbounds = { 
        {
            chain = { 
                { 
                    Fileio={ 
                          i= "local.suit.toml", o = "testfile.txt", 
                          sleep_interval = 500, bytes_per_turn = 10, 
                            ext = { fixed_target_addr= "fake.com:80" } 
                    } 
                }  
            } , tag = "listen1"
        } ,
    },

    outbounds = { { tag="dial1", chain = dial_trojan_chain } }
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



--[=[

config = {
    inbounds = { 
        {
            tag = "in_stdio_adder_chain",

            chain =  { 
                { 
                    Stdio={ 
                        fixed_target_addr= "udp://127.0.0.1:20800", 
                        pre_defined_early_data = "abc" 
                    } 
                } , 
                { Adder = 1 } 
            }
        } , 
    } ,
    outbounds = { 
        { tag="d1", chain = { dial, { Socks5 = {} } } } , 
    },

--[[
这个 config 块是演示 试图用 socks5 客户端向 一个 本地 udp 监听发起请求.

该配置对应的 remote.lua 的 配置应该是 socks5 监听 -> direct
--]]

}

--]=]



--[=[
config = {

    inbounds = { 
        {chain = { { Dialer="ip://10.0.0.1:24#utun321" } }, tag = "listen1"} ,
    },

--[[
这个 config 块是演示 inbound 是 ip, outbound 是stdio的情况, 

此时需要注意, 该配置下 要用 sudo 运行, 且 rucimp 的 "tun" feature 是打开的

--]]

    outbounds = { { tag="dial1", chain = out_stdio_chain } }
}

--]=]
