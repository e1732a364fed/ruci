print("this is a lua local config file")

-- lua 的好处有很多, 你可以定义很多变量
-- 真正的配置块是 接近文件底部的 Config 变量, 可以用搜索快速找到它

local listen = {
    Listener = { listen_addr = "0.0.0.0:10800" }
}
local l2 = {
    Listener = {
        listen_addr = "udp://0.0.0.0:20800",

        ---[[

        -- 如果 ext 中的 fixed_target_addr 给出, 则其行为等价于
        -- 一些其它代理程序中 所定义的 "dokodemo door (任意门)"

        -- ruci 中, Listener,TcpOptListener, BindDialer, Stdio, Fileio 都能如此配置

        ext = {
            fixed_target_addr = "udp://114.114.114.114:53"
            --fixed_target_addr = "1.1.1.1:80" -- 不给://时 默认为 tcp
        }
        --]]
    }
}
local l3 = {
    Listener = { listen_addr = "[::1]:30800" }
}

local tproxy_tcp_listen = {
    TcpOptListener = {
        listen_addr = "0.0.0.0:12345",
        sockopt = {
            tproxy = true,
        }
    }
}

local tproxy_udp_listen = {
    TproxyUdpListener = {
        listen_addr = "udp://0.0.0.0:12345",
        sockopt = {
            tproxy = true,
        }
    }
}

local listen_socks5 = { listen, {
    Socks5 = {}
} }
local listen_http = { listen, {
    Http = {}
} }
local listen_socks5http = { listen, {
    Socks5Http = {}
} }

local tproxy_listen_tcp_chain = {
    tproxy_tcp_listen, {
    TproxyTcpResolver = {
        port = 12345,
        --auto_route_tcp = true, -- only set route for tcp
        auto_route = true,         -- auto_route will set route for both tcp and udp at the appointed port

        route_ipv6 = true,         -- 如果为true, 则  也会 对 ipv6 网段执行 自动路由

        proxy_local_udp_53 = true, -- 如果为true, 则 udp 53 端口不会直连, 而是会流经 tproxy

        -- local_net4 = "192.168.0.0/16" -- 直连 ipv4 局域网段 不给出时, 默认即为 192.168.0.0/16
    }
}
}

local opt_direct_chain = {
    {
        OptDirect = {
            so_mark = 255,
            bind_to_device = "enp0s1"
        }
    }
}

local tlsout = {
    -- NativeTLS = {
    TLS = {
        host = "www.1234.com",
        insecure = true
        -- alpn = {"http"}

    }
}

local tlsin = {
    TLS = {
        cert = "test.crt",
        key = "test.key"
    }
}

local trojan_in = {
    Trojan = {
        password = "mypassword"
    }
}

local listen_trojan = { listen, trojan_in }

local dial = {
    BindDialer = {
        dial_addr = "tcp://0.0.0.0:10801"
    }
}

local opt_dial = {
    OptDialer = {
        dial_addr = "tcp://0.0.0.0:10801", -- 用tproxy时, 不能为 127.0.0.1, 须为 0.0.0.0
        sockopt = {
            so_mark = 255,
            bind_to_device = "enp0s1"
        }
    }
}


local trojan_out = {
    Trojan = "mypassword"
}

-- http 请求 (ws,h2 有用到)中的 authority 会被填到
-- 实际 http/1.1 请求 中的 Host header中 和 h2 请求中的  Request Pseudo-Header Fields 中的 authority 中,
-- 之所以不叫它 host 是因为它是可以包含端口号的

local websocket_out = {
    WebSocket = {
        authority = "myhost",
        path = "/path1",
        use_early_data = true
    }
}

local dial_trojan_chain = { dial, tlsout, trojan_out }
local opt_dial_trojan_chain = { opt_dial, tlsout, trojan_out }
local dial_ws_trojan_chain = { dial, tlsout, websocket_out, trojan_out }

local h2_single_out = {
    H2Single = {
        is_grpc = true,
        http_config = {
            authority = "myhost",
            path = "/service1/Tun"
        }
    }
}

local quic_out_chain = { {
    Quic = {
        --is_insecure = true,

        -- 可给出 服务端的 证书, 这样就算 is_insecure = false 也通过验证
        -- 证书须为 真证书, 或真fullchain 证书, 或自签的根证书
        cert_path = "test2.crt",
        server_addr = "127.0.0.1:10801",

        -- 须给出 server_name,
        --  且 若 is_insecure 为 false, 须为 证书中所写的 CN 或 Subject Alternative Name;
        -- ruci 提供的 test2.crt中的 Subject Alternative Name 为 www.mytest.com 和 localhost,

        server_name = "www.mytest.com",

        alpn = { "h3" } --要明确指定 alpn
    }
}, trojan_out }

local dial_h2_trojan_chain = { dial, tlsout, h2_single_out, trojan_out }

local stdio_socks5_chain = { {
    Stdio = {}
}, {
    Socks5 = {}
} }

-- stdin + 1 , 在命令行输入 a, 会得到b, 输入1, 得2, 依此类推
-- 设了 abc 为预先信息, 刚连上后就会发出abc 信号
local in_stdio_adder_chain = { {
    Stdio = {
        pre_defined_early_data = "abc"
    }
}, {
    Adder = 1
} }

local out_stdio_chain = { {
    Stdio = {}
} }

local direct_out_chain = { "Direct" }

---[=[

local config_1_direct = {
    inbounds = { {
        chain = listen_socks5http,
        tag = "listen1"
    } },
    outbounds = { {
        tag = "dial1",
        chain = { "Direct" }
    } }

    --[[
演示 inbound 是 socks5http, outbound 是 direct 的情况

它是一个基本的本地代理示例. 运行它, 设置您的系统代理为相应端口, 看看能不能正常访问网络吧
--]]

}

-- ]=]



---[=[

local config_2_tproxy1 = {
    inbounds = { {
        chain = tproxy_listen_tcp_chain,
        tag = "listen1"
    },
        {
            chain = { tproxy_udp_listen },
            tag = "listen_udp1"
        }
    },
    outbounds = { {
        tag = "direct",
        chain = opt_direct_chain
    } },

    tag_route = { { "listen1", "direct" }, { "listen_udp1", "direct" } },

    --[[
演示 inbound 是 tproxy, outbound 是 direct 的情况
注意 direct 用的是 opt_direct, 用了 somark 和 bind_to_device
透明代理tproxy 只能在 linux 上使用.
--]]

}

-- ]=]

---[=[

local config_3_tproxy2 = {
    inbounds = { {
        chain = tproxy_listen_tcp_chain,
        tag = "listen1"
    },
        {
            chain = { tproxy_udp_listen },
            tag = "listen_udp1"
        }
    },
    outbounds = { {
        tag = "out",
        chain = opt_dial_trojan_chain
    } },

    tag_route = { { "listen1", "out" }, { "listen_udp1", "out" } },

    --[[
演示 inbound 是 tproxy, outbound 是  trojan out 的情况
透明代理tproxy 只能在 linux 上使用.

另外, 如果在 listen tproxy 的同一主机上 监听 trojan ,即同一电脑上运行 remote.lua 中的 对应配置,

对应配置中是不需要再用 "TcpOptListener" 的, 直接正常监听就行, 但其direct 要为 OptDialer 并给出 somark 和 bind_to_device

--]]

}

-- ]=]


---[=[

local config_4_trojan = {
    inbounds = { { chain = listen_socks5http, tag = "listen1" } },
    outbounds = { { tag = "dial1", chain = dial_trojan_chain } }

    --[[
演示 inbound 是 socks5http, outbound 是 trojan+tls 的情况

它是一个基本的远程代理示例. 运行它, 设置您的系统代理为相应端口,
并参照 remote.lua 在另一个终端 运行 另一部分,
看看能不能正常访问网络吧
--]]

}

--]=]

---[=[

local config_5_unix = {
    inbounds = { { chain = listen_socks5http, tag = "listen1" } },
    outbounds = {
        {
            tag = "dial1",
            chain = { { BindDialer = { dial_addr = "unix://file1" } }, tlsout, trojan_out }
        }
    }

    --[[
与上面的 示例类似, 但是它 的 dial 是用的 unix domain socket
与此对应的 remote.lua 中 也应该是 unix 的监听

--]]

}

--]=]

---[=[
local config_6_ws = {
    inbounds = { { chain = listen_socks5http, tag = "listen1" } },
    outbounds = { { tag = "dial1", chain = dial_ws_trojan_chain } },

    -- 演示 inbound 是 socks5http, outbound 是 tcp+tls+ws+trojan 的情况


}

--]=]

---[=[
local config_7_h2 = {
    inbounds = { { chain = listen_socks5http, tag = "listen1" } },
    outbounds = { { tag = "dial1", chain = dial_h2_trojan_chain } },

    -- 演示 inbound 是 socks5http, outbound 是 tcp+tls+h2+trojan 的情况
    -- (非多路复用. mux的情况见 local_mux_h2.lua 和 local_mux2_h2.lua)
}

--]=]

---[=[
local config_8_quic = {
    inbounds = { {
        chain = listen_socks5http,
        tag = "listen1"
    } },
    outbounds = { {
        tag = "dial1",
        chain = quic_out_chain
    } }

    -- 演示 inbound 是 socks5http, outbound 是 quic 的情况
}

-- ]=]

---[=[
local config_9_stdin_adder = {

    inbounds = {
        { chain = in_stdio_adder_chain, tag = "listen1" },
    },

    --[[
演示 inbound 是 stdio (命令行)+1, outbound 也是stdio的情况,

此时需要注意, 该配置下 命令行 的输入会既用作 inbound 的输入, 也用作 outbound 的输入;

在实际操作中, 您会看到, 输入被in和out轮流使用, 因此会有 一次+1, 一次不+1的情况轮流出现

--]]

    outbounds = { { tag = "dial1", chain = out_stdio_chain } }
}

--]=]

---[[


local config_10_stdin_trojan = {

    -- stdin + 1 -> trojan_out

    inbounds = {
        { chain = in_stdio_adder_chain, tag = "listen1" },
    },

    outbounds = { { tag = "dial1", chain = dial_trojan_chain } }
}

--]]

---[[


local config_11_stdin_blackhole = {

    -- stdin + 1 -> blackhole

    inbounds = {
        { chain = in_stdio_adder_chain, tag = "listen1" },
    },

    --expected warn: dial out client stream got consumed

    outbounds = { { tag = "dial1", chain = { "Blackhole" } } }
}

--]]

---[[


local config_12_fileio_trojan = {

    -- fileio -> trojan_out

    inbounds = {
        {
            chain = {
                {
                    Fileio = {
                        i = "local.suit.toml",
                        o = "testfile.txt",
                        sleep_interval = 500,
                        bytes_per_turn = 10,
                        ext = { fixed_target_addr = "fake.com:80" }
                    }
                }
            },
            tag = "listen1"
        },
    },

    outbounds = { { tag = "dial1", chain = dial_trojan_chain } }
}

--]]

---[=[

local config_13_route = {
    inbounds = {
        {
            chain = listen_socks5http,
            tag = "l1"
        },

        {
            -- 测试: dig @127.0.0.1 -p 20800 www.baidu.com

            chain = {
                l2, -- 多客户端连接的情况
                --[[
            {
                -- 只允许单客户端连接的情况

                BindDialer = {
                    bind_addr = "udp://127.0.0.1:20800",
                    ext = {
                        fixed_target_addr = "udp://114.114.114.114:53"
                    }
                }
            }
            --]]

            },
            tag = "l2"
        }, {
        chain = { l3, tlsin },
        tag = "l3"
    } },
    outbounds = { {
        tag = "d1",
        chain = { "Direct" }
    }, {
        tag = "d2",
        chain = dial_trojan_chain
    }, {
        tag = "fallback_d",
        chain = { {
            BindDialer = { dial_addr = "tcp://0.0.0.0:80" }
        } }
    } },

    ---[==[
    tag_route = { { "l1", "d1" }, { "l2", "d2" }, { "l3", "d2" } },

    fallback_route = { { "l1", "fallback_d" } }

    -- ]==]

    --[==[

    rule_route = {{
        mode = "WhiteList",
        out_tag = "d1",
        in_tags = {"l1"}
    }, {
        mode = "WhiteList",
        out_tag = "d2",
        in_tags = {"l3","l2"}
    }, {
        mode = "WhiteList",
        out_tag = "fallback_d",
        in_tags = {"l1"},
        is_fallback = true
    }}

    -- ]==]

    --[[
演示 多in多out的情况, 只要outbounds有多个, 您就应该考虑使用路由配置

路由同时给出了 使用 tag_route + fallback_route 的 简单配置 和用 rule_route 的复杂配置

这两种给出的配置在行为上是等价的

该 路由 示例明确指出, l1将被路由到d1, l2 -> d2, l3 -> d2, 且 l1 的回落为 fallback_d

--]]

}

-- ]=]

---[=[

local config_14_stdio_adder_udp_fixed_target_addr = {
    inbounds = {
        {
            tag = "in_stdio_adder_chain",

            chain = {
                {
                    Stdio = {
                        fixed_target_addr = "udp://127.0.0.1:20800",
                        pre_defined_early_data = "abc"
                    }
                },
                { Adder = 1 }
            }
        },
    },
    outbounds = {
        { tag = "d1", chain = { dial, { Socks5 = {} } } },
    },

    --[[
演示 试图用 socks5 客户端向 一个 本地 udp 监听发起请求.

该配置对应的 remote.lua 的 配置应该是 socks5 监听 -> direct
--]]

}

--]=]

---[=[
local config_15_tun = {

    inbounds = {

        --这里的 "24" 不是端口, 因为 ip 协议没有 端口的说法; 24 是 子网掩码的 CIDR 表示法,
        -- 表示 255.255.255.0; ruci这里采用与 tcp 端口写法一致的格式, 便于处理

        { chain = { { BindDialer = { bind_addr = "ip://10.0.0.1:24#utun321" } } }, tag = "listen1" },
    },

    --[[
演示 inbound 是 ip, outbound 是stdio的情况,

此时需要注意, 该配置下 要用 sudo 运行, 且 rucimp 的 "tun" feature 是打开的

它会建一个 叫 utun321 的 utun 虚拟网卡, 然后 ruci 会监听 其 网卡的 10.0.0.1

--]]

    outbounds = { { tag = "dial1", chain = out_stdio_chain } }
}

--]=]

Config = config_2_tproxy1

---[[

-- 有限动态链的 选择器用法 的基本演示
-- 有限动态链使用 Config 所提供的列表, 在 Dyn_Selectors 中动态地
-- 根据参数 返回列表的索引值
-- 下面 示例是 最简单的示例, 使得动态链的行为和静态链相同

function Dyn_Selectors(tag)
    return function(this_index, data)
        -- print("data:",data)

        return this_index + 1
    end
end

-- ]]

---[[

-- 完全动态链的基本演示

-- 完全动态链不使用 固定的列表 来预定义任何Maps, 它只给出一个函数
-- generator, generator 根据参数内容来动态生成 [Map], 如果不想
-- 重复生成以前生成过的Map, 则可以返回一个已创建过的Map (参见其它包含 infinite 的配置文件中的示例)

-- local inspect = require("inspect")

-- my_cid_record = {}

Infinite = {

    -- 下面这个演示 与第一个普通示例 行为上等价

    inbounds = { {
        tag = "listen1",

        generator = function(cid, state_index, data)
            if state_index == -1 then
                return 0, {
                    stream_generator = {
                        Listener = { listen_addr = "0.0.0.0:10800" }
                    },
                    new_thread_fn = function(cid, state_index, data)
                        -- print("lua: cid",inspect(cid))
                        -- table.insert(my_cid_record,cid)
                        -- print("lua: cid cache",inspect(my_cid_record))

                        local new_cid, newi, new_data = coroutine.yield(1, {
                            Socks5 = {}
                        })
                        return -1, {}
                    end
                }
            end
        end
    } },

    outbounds = { {
        tag = "dial1",
        generator = function(cid, state_index, data)
            if state_index == -1 then
                return 0, "Direct"
            else
                return -1, {}
            end
        end
    } }

}

-- ]]
