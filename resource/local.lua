print("this is a lua config file")


-- lua 的好处有很多，你可以定义很多变量
-- 真正的配置块是 config 变量，可以用搜索快速找到它

listen = {
    Listener = "0.0.0.0:10800"
}
l2 = {
    Listener = "0.0.0.0:20800"
}
l3 = {
    Listener = "0.0.0.0:30800"
}

listen_socks5 = {listen, {
    Socks5 = {}
}}
listen_http = {listen, {
    Http = {}
}}
listen_socks5http = {listen, {
    Socks5Http = {}
}}

tlsout = {
    TLS = {
        host = "www.1234.com",
        insecure = true
    }
}

tlsin = {
    TLS = {
        cert = "test.crt",
        key = "test.key"
    }
}

trojan_in = {
    Trojan = {
        password = "mypassword"
    }
}

listen_trojan = {listen, trojan_in}

dial = {
    Dialer = "tcp://0.0.0.0:10801"
}

trojan_out = {
    Trojan = "mypassword"
}

websocket_out = {
    WebSocket ={
        host = "myhost",
        path = "/path1",
        use_early_data = true
    }
}

dial_trojan_chain = {dial, tlsout, trojan_out}
dial_trojan_ws_chain = {dial,tlsout, websocket_out, trojan_out}

stdio_socks5_chain = {{
    Stdio = {}
}, {
    Socks5 = {}
}}

-- stdin + 1 , 在命令行输入 a, 会得到b，输入1，得2，依此类推
-- 设了 abc 为预先信息, 刚连上后就会发出abc 信号
in_stdio_adder_chain = {{
    Stdio = {
        pre_defined_early_data = "abc"
    }
}, {
    Adder = 1
}}

out_stdio_chain = {{
    Stdio = {}
}}

direct_out_chain = {"Direct"}

--[=[

config = {
    inbounds = {{
        chain = listen_socks5http,
        tag = "listen1"
    }},
    outbounds = {{
        tag = "dial1",
        chain = {"Direct"}
    }}

    --[[
这个 config 块是演示 inbound 是 socks5http, outbound 是 direct 的情况

它是一个基本的本地代理示例. 运行它, 设置您的系统代理为相应端口, 看看能不能正常访问网络吧
--]]

}

-- ]=]

--[=[
-- default counterpart for remote.lua

config = {
    inbounds = { {chain = listen_socks5http, tag = "listen1"} },
    outbounds = { { tag="dial1", chain = dial_trojan_chain } }

--[[
这个 config 块是演示 inbound 是 socks5http, outbound 是 trojan+tls 的情况

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


---[=[
config = {
    inbounds = { {chain = listen_socks5http, tag = "listen1"} },
    outbounds = { { tag="dial1", chain = dial_trojan_ws_chain } }

--[[
这个 config 块是演示 inbound 是 socks5http, outbound 是 trojan+tls+ws 的情况
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
        { tag="d1", chain = { "Direct" } } ,  { tag="d2", chain = { dial, tlsout } } ,
        {
            tag = "fallback_d", chain = {
                Dialer = "tcp://0.0.0.0:80"
            }
        }
    },

    tag_route = {  { "l1","d1" },{ "l2","d2"},{"l3","d2"} },

    fallback_route = { {  "l1", "fallback_d" } }

--[[
这个 config 块是演示 多in多out的情况, 只要outbounds有多个，您就应该考虑使用路由配置
该 tag_route 示例明确指出，l1将被路由到d1, l2 -> d2, l3 -> d2

l1 的回落为 fallback_d
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

        --这里的 "24" 不是端口, 因为 ip 协议没有 端口的说法; 24 是 子网掩码的 CIDR 表示法,
        -- 表示 255.255.255.0; ruci这里采用与 tcp 端口写法一致的格式, 便于处理

        {chain = { { Dialer="ip://10.0.0.1:24#utun321" } }, tag = "listen1"} ,
    },

--[[
这个 config 块是演示 inbound 是 ip, outbound 是stdio的情况, 

此时需要注意, 该配置下 要用 sudo 运行, 且 rucimp 的 "tun" feature 是打开的

它会建一个 叫 utun321 的 utun 虚拟网卡, 然后 ruci 会监听 其 网卡的 10.0.0.1 

--]]

    outbounds = { { tag="dial1", chain = out_stdio_chain } }
}

--]=]

---[[

-- 有限动态链的 选择器用法 的基本演示 
-- 有限动态链使用 config 所提供的列表, 在 *_next_selector 中动态地
-- 根据参数 返回列表的索引值

function dyn_selectors(tag)
    if tag == "listen1" then
        return dyn_inbound_next_selector
    end
    if tag == "dial1" then
        return dyn_outbound_next_selector
    end
end

-- 下面两个selector 示例都是 最简单的示例, 使得动态链的行为和静态链相同

dyn_inbound_next_selector = function(this_index, data)
    -- print("data:",data)

    return this_index + 1
end

dyn_outbound_next_selector = function(this_index, ovov)
    return this_index + 1
end

-- ]]

---[[

-- 完全动态链的基本演示

-- 完全动态链不使用 固定的列表 来预定义任何Mappers, 它只给出一个函数
-- generator, generator 根据参数内容来动态生成 [Mapper], 如果不想
-- 重复生成以前生成过的Mapper, 则可以返回一个已存在的索引

local inspect = require("inspect")

--my_cid_record = {}

infinite = {

    -- 下面这个演示 与第一个普通示例 形式上等价

    inbounds = {{
        tag = "listen1",

        generator = function(cid, this_index, data)
            if this_index == -1 then
                return 0, {
                    stream_generator = {
                        Listener = "0.0.0.0:10800"
                    },
                    new_thread_fn = function(cid, this_index, data)
                        --print("lua: cid",inspect(cid))
                        --table.insert(my_cid_record,cid)
                        --print("lua: cid cache",inspect(my_cid_record))

                        local new_cid, newi, new_data = coroutine.yield(1, {
                            Socks5 = {}
                        })
                        return -1, {}
                    end
                }
            end
        end
    }},

    outbounds = {{
        tag = "dial1",
        generator = function(cid, this_index, data)
            if this_index == -1 then
                return 0, "Direct"
            else
                return -1, {}
            end
        end
    }}

}

-- ]]
