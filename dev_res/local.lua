print("this is a lua local config file")

-- lua 的好处有很多, 你可以定义很多变量
-- 真正的配置块是 接近文件底部的 Config 变量, 可以用搜索快速找到它

local listen_10800 = {
    type = "Listener", listen_addr = "0.0.0.0:10800"
}
local listen_fixed_target = {
    type = "Listener",
    listen_addr = "udp://0.0.0.0:20800",

    ---[[

    -- 如果 ext 中的 fixed_target_addr 给出, 则其行为等价于
    -- 一些其它代理程序中 所定义的 "dokodemo door (任意门)"

    -- ruci 中, Listener,TcpOptListener, BindDialer, Stdio, Fileio 都能如此配置

    ext = {
        fixed_target_addr = "udp://8.8.8.8:53"
        --fixed_target_addr = "1.1.1.1:80" -- 不给://时 默认为 tcp
    }
    --]]

}
local listen_ipv6 = {
    type = "Listener", listen_addr = "[::1]:30800"
}

local tproxy_tcp_listen = {
    type = "TcpOptListener",
    listen_addr = "0.0.0.0:12345",
    sockopt = {
        tproxy = true,
    }

}

local tproxy_udp_listen = {
    type = "TproxyUdpListener",
    listen_addr = "udp://0.0.0.0:12345",
    sockopt = {
        tproxy = true,
    }

}

local listen_socks5 = { listen_10800, {
    type = "Socks5"
} }
local listen_http = { listen_10800, {
    type = "Http"
} }
local listen_socks5http = { listen_10800, {
    type = "Socks5Http"
} }

local tproxy_listen_tcp_chain = {
    tproxy_tcp_listen, {
    type = "TproxyTcpResolver",
    port = 12345,
    --auto_route_tcp = true, -- only set route for tcp
    auto_route = true,         -- auto_route will set route for both tcp and udp at the appointed port

    route_ipv6 = true,         -- 如果为true, 则  也会 对 ipv6 网段执行 自动路由

    proxy_local_udp_53 = true, -- 如果为true, 则 udp 53 端口不会直连, 而是会流经 tproxy

    -- local_net4 = "192.168.0.0/16" -- 直连 ipv4 局域网段 不给出时, 默认即为 192.168.0.0/16

}
}

local opt_direct_chain = {
    {
        type = "OptDirect",
        sockopt = {
            so_mark = 255,
            bind_to_device = "wlp3s0" --"en0" --"enp0s1"
        }

    }
}

local tlsout = {
    -- NativeTLS = {
    type = "TLS",
    host = "www.1234.com",
    insecure = true
    -- alpn = {"http/1.1"}


}

local tlsin = {
    type = "TLS",
    cert = "test.crt",
    key = "test.key"

}

local trojan_in = {
    type = "Trojan",
    password = "mypassword"
}

local listen_trojan = { listen_10800, trojan_in }

local dial = {
    type = "BindDialer",
    dial_addr = "tcp://127.0.0.1:10801"
}

---[[
-- 在本示例中 tproxy 是单机自连测试, 因此没有用到 OptDialer
-- 在实际使用中, 如果是dial 一个真实的远程服务器, 需要用 OptDialer
-- 加 so_mark 和 bind_to_device

local opt_dial = {
    type = "OptDialer",
    dial_addr = "tcp://192.168.0.202:10801", --"tcp://127.0.0.1:10801",
    sockopt = {
        so_mark = 255,
        bind_to_device = "wlp3s0" --"enp0s1"

    }
}


--]]


local trojan_out = {
    type = "Trojan",
    password = "mypassword",

}

-- http 请求 (ws,h2 有用到)中的 authority 会被填到
-- 实际 http/1.1 请求 中的 Host header中 和 h2 请求中的  Request Pseudo-Header Fields 中的 authority 中,
-- 之所以不叫它 host 是因为它是可以包含端口号的

local websocket_out = {
    type = "WebSocket",
    authority = "myhost",
    path = "/path1",
    use_early_data = true

}

local dial_trojans_chain = { dial, tlsout, trojan_out }
local optdial_trojans_chain = { opt_dial, tlsout, trojan_out }

local dial_ws_trojans_chain = { dial, tlsout, websocket_out, trojan_out }

local h2_single_out = {
    type = "H2Single",
    is_grpc = true,
    http_config = {
        authority = "myhost",
        path = "/service1/Tun"
    }

}

local quic_out_chain = { {
    type = "Quic",
    --insecure = true,

    -- 可给出 服务端的 证书, 这样就算 insecure = false 也通过验证
    -- 证书须为 真证书, 或真fullchain 证书, 或自签的根证书
    cert = "test2.crt",
    server_addr = "127.0.0.1:10801",

    -- 须给出 server_name,
    --  且 若 insecure 为 false, 须为 证书中所写的 CN 或 Subject Alternative Name;
    -- ruci 提供的 test2.crt中的 Subject Alternative Name 为 www.mytest.com 和 localhost,

    server_name = "www.mytest.com",

    alpn = { "h3" } --要明确指定 alpn

}, trojan_out }

local dial_h2_trojan_chain = { dial, tlsout, h2_single_out, trojan_out }

local stdio_socks5_chain = { {
    type = "Stdio"
}, {
    type = "Socks5"
} }

-- stdin + 1 , 在命令行输入 a, 会得到b, 输入1, 得2, 依此类推
-- 设了 abc 为预先信息, 刚连上后就会发出abc 信号
local in_stdio_adder_chain = { {
    type = "Stdio",
    ext = {
        pre_defined_early_data = "abc"
    }

}, {
    type = "Adder", value = 1
} }

local out_stdio_chain = { {
    type = "Stdio"
} }

local out_stdio_show_bytes_chain = { {
    type = "Stdio",
    write_mode = "Bytes" -- 默认的 write_mode 为 UTF8, 可以用 Bytes 模式来观察16进制数据

} }

local direct = { type = "Direct" }

-- 该配置 和 listen_fixed_target 联动 (127.0.0.1:20800, 指定本地地址时不要写0.0.0.0， 否则会卡住)
-- 该配置 会 在该 Direct 所属的 chain 中创建一个 新的 dns client, 对于 域名请求将使用 指定的
-- dns_server 来 解析. 注意这里 dns_server 就不要再用 域名了，否则就会造成无限循环
local direct_with_dns = {
    type = "Direct",
    dns_client = {
        dns_server_list = { { "127.0.0.1:20800", "udp" } }, -- 8.8.8.8:53
        ip_strategy = "Ipv4Only",
        static_pairs = {
            ['www.baidu.com'] = "103.235.47.188"
        }
    }

}

local config_0_direct = {
    inbounds = {
        listen1 = listen_socks5http,
    },
    outbounds = {
        dial1 = { direct }
    }

    --[[
演示 inbound 是 socks5http, outbound 是 direct 的情况

它是一个基本的本地代理示例. 运行它, 设置您的系统代理为相应端口, 看看能不能正常访问网络吧
--]]

}

local config_1_direct_dns = {
    inbounds = {
        listen1 = listen_socks5http,
        listen2 = { listen_fixed_target },
    },

    outbounds = {
        dial1 = { direct_with_dns }
    }

}

local tproxy_listen_inbounds = {
    listen1 = tproxy_listen_tcp_chain,
    listen_udp1 = { tproxy_udp_listen },
}

local config_2_tproxy1 = {
    inbounds = tproxy_listen_inbounds,
    outbounds = {
        direct = opt_direct_chain
    },
    routes = {
        tag_route = { { "listen1", "direct" }, { "listen_udp1", "direct" } },
    }

    --[[
演示 inbound 是 tproxy, outbound 是 direct 的情况
注意 direct 用的是 opt_direct, 用了 somark 和 bind_to_device
透明代理tproxy 只能在 linux 上使用.
--]]

}

local config_3_tproxy2 = {
    inbounds = tproxy_listen_inbounds,
    outbounds = {
        out = optdial_trojans_chain
    },

    routes = {
        tag_route = { { "listen1", "out" }, { "listen_udp1", "out" } },
    }

    --[[
演示 inbound 是 tproxy, outbound 是  trojan out 的情况
透明代理tproxy 只能在 linux 上使用.

另外, 如果在 listen tproxy 的同一主机上 监听 trojan ,即同一电脑上运行 remote.lua 中的 对应配置,

对应配置中是不需要再用 "TcpOptListener" 的, 直接正常监听就行, 但其direct 要为 OptDialer 并给出 somark 和 bind_to_device

--]]

}


local config_4_trojans = {
    inbounds = {
        listen1 = listen_socks5http,
        listen2 = { listen_fixed_target },
    },
    outbounds = { dial1 = dial_trojans_chain }

    --[[
演示 inbound 是 socks5http, outbound 是 trojan+tls 的情况

它是一个基本的远程代理示例. 运行它, 设置您的系统代理为相应端口,
并参照 remote.lua 在另一个终端 运行 另一部分,
看看能不能正常访问网络吧
--]]

}

local config_5_unix = {
    inbounds = { listen1 = listen_socks5http },
    outbounds = {
        dial1 = { { type = "BindDialer", dial_addr = "unix://file1" }, tlsout, trojan_out }
    }

    --[[
与上面的 示例类似, 但是它 的 dial 是用的 unix domain socket
与此对应的 remote.lua 中 也应该是 unix 的监听

--]]

}

local config_6_ws = {
    inbounds = { listen1 = listen_socks5http },
    outbounds = { dial1 = dial_ws_trojans_chain },
    -- 演示 inbound 是 socks5http, outbound 是 tcp+tls+ws+trojan 的情况
}

local config_7_h2 = {
    inbounds = { listen1 = listen_socks5http },
    outbounds = { dial1 = dial_h2_trojan_chain },

    -- 演示 inbound 是 socks5http, outbound 是 tcp+tls+h2+trojan 的情况
    -- (非多路复用. mux的情况见 local_mux_h2.lua 和 local_mux2_h2.lua)
}

local config_8_quic = {
    inbounds = {
        listen1 = listen_socks5http,
    },
    outbounds = {
        dial1 = quic_out_chain
    }

    -- 演示 inbound 是 socks5http, outbound 是 quic 的情况
}

local config_9_stdio_adder = {

    inbounds = {
        listen1 = in_stdio_adder_chain,
    },

    --[[
演示 inbound 是 stdio (命令行)+1, outbound 也是stdio的情况,

此时需要注意, 该配置下 命令行 的输入会既用作 inbound 的输入, 也用作 outbound 的输入;

在实际操作中, 您会看到, 输入被in和out轮流使用, 因此会有 一次+1, 一次不+1的情况轮流出现

--]]

    outbounds = { dial1 = out_stdio_chain }
}

local config_10_stdin_trojan = {

    -- stdin + 1 -> trojan_out

    inbounds = {
        listen1 = in_stdio_adder_chain,
    },

    outbounds = { dial1 = dial_trojans_chain }
}


local config_11_stdin_blackhole = {

    -- stdin + 1 -> blackhole

    inbounds = {
        listen1 = in_stdio_adder_chain,
    },

    --expected warn: dial out client stream got consumed

    outbounds = { dial1 = { "Blackhole" } }
}


local config_12_fileio_trojan = {

    -- fileio -> trojan_out

    inbounds = {
        listen1 = {
            {
                type = "Fileio",
                i = "test.crt",
                o = "testfile.txt",
                sleep_interval = 500,
                bytes_per_turn = 10,
                ext = { fixed_target_addr = "fake.com:80" }

            }
        },
    },

    outbounds = { dial1 = dial_trojans_chain }
}


local config_13_route = {
    inbounds = {
        l1 = listen_socks5http,

        -- 测试: dig @127.0.0.1 -p 20800 www.baidu.com

        l2 = {
            listen_fixed_target, -- fixed_target_addr udp 为 将被多客户端连接 的情况
            --[[
            {
                -- 只允许单客户端连接 该 fixed_target_addr udp 的情况(仅供测试使用)

                BindDialer = {
                    bind_addr = "udp://127.0.0.1:20800",
                    ext = {
                        fixed_target_addr = "udp://114.114.114.114:53"
                    }
                }
            }
            --]]

        },
        l3 = { listen_ipv6, tlsin },
    },
    outbounds = {
        d1 = { direct },
        d2 = dial_trojans_chain,
        fallback_d = { {
            type = "BindDialer", dial_addr = "tcp://127.0.0.1:80"
        } }
    },

    routes = {
        --[==[
        tag_route = { { "l1", "d1" }, { "l2", "d2" }, { "l3", "d2" } },

        fallback_route = { { "l1", "fallback_d" } }

        -- ]==]

        ---[==[

        rule_route = { {
            mode = "WhiteList",
            out_tag = "d1",
            in_tags = { "l1" }
        }, {
            mode = "WhiteList",
            out_tag = "d2",
            in_tags = { "l3", "l2" }
        }, {
            mode = "WhiteList",
            out_tag = "fallback_d",
            in_tags = { "l1" },
            is_fallback = true
        } }

        -- ]==]
    }


    --[[
演示 多in多out的情况, 只要outbounds有多个, 您就应该考虑使用路由配置

路由同时给出了 使用 tag_route + fallback_route 的 简单配置 和用 rule_route 的复杂配置

这两种给出的配置在行为上是等价的

该 路由 示例明确指出, l1将被路由到d1, l2 -> d2, l3 -> d2, 且 l1 的回落为 fallback_d

--]]

}

local config_14_stdio_adder_udp_fixed_target_addr = {
    inbounds = {
        in_stdio_adder_chain = {
            {
                type = "Stdio",
                ext = {
                    fixed_target_addr = "udp://127.0.0.1:20800",
                    pre_defined_early_data = "abc"

                }
            },
            { type = "Adder", value = 1 }
        },
    },
    outbounds = {
        d1 = { dial, { type = "Socks5" } },
    },

    --[[
演示 试图用 socks5 客户端向 一个 本地 udp 监听发起请求.

该配置对应的 remote.lua 的 配置应该是 socks5 监听 -> direct
--]]

}

local config_15_tun_stdio_out = {

    inbounds = {
        listen1 = { {
            type = "BindDialer",


            --这里的 "24" 不是端口, 因为 ip 协议没有 端口的说法; 24 是用的 子网掩码的 CIDR 表示法,
            -- 表示 255.255.255.0; ruci这里采用与 tcp 端口写法一致的格式, 便于处理

            bind_addr = "ip://10.0.0.1:24#utun321",

            -- 自动配置 系统路由 以 代理全局
            in_auto_route = {
                tun_dev_name = "utun321",
                tun_gateway = "10.0.0.1",
                router_ip = "192.168.0.1",
                original_dev_name = "enp0s1",
                dns_list = { "1.1.1.1" }
            }
        } },
    },
    outbounds = { dial1 = out_stdio_show_bytes_chain }

    --[[

        演示 inbound 是 ip, outbound 是stdio的情况, 即把 tun 收到的 ip 信息打印在命令行中

        此时需要注意, 该配置下 要用 sudo 运行, 且 rucimp 的 "tun" feature 是打开的

        它会建一个 叫 utun321 的 utun 虚拟网卡, 然后 ruci 会监听 其 网卡的 10.0.0.1

        用了自动路由, 这样 全局的流量都会打印在 命令行中 (但也只会打印在命令行中, 该配置中 没有转发到别处)

    --]]

}

local config_16_tun = {

    inbounds = {
        listen1 = { {
            type = "BindDialer",
            bind_addr = "ip://10.0.0.1:24#utun321",

            in_auto_route = {
                tun_dev_name = "utun321",
                tun_gateway = "10.0.0.1",
                router_ip = "192.168.0.1",
                original_dev_name = "enp0s1", -- windows/macos 可不填 original_dev_name, linux 要填 original_dev_name
                --direct_list = { "192.168.0.204" }, -- 服务端的ip要直连
                dns_list = { "114.114.114.114" }

            }
        } },
    },
    outbounds = {
        dial1 = { {
            type = "OptDialer", -- 如果自动路由没写 direct_list, 也可以用 OptDialer+ bind_to_device 的方法

            -- 注: windows 上要用 OptDialer + bind_to_device 的方法

            -- BindDialer = {
            dial_addr = "tcp://192.168.0.204:10801",
            sockopt = {
                bind_to_device = "en0"

                -- enp0s1(linux 的一般情况)
                -- en0  (macos 的情况)
                -- WLAN( windows, 英文系统 用wifi联网的情况) (windows中的网卡信息使用 ipconfig 查看)
                -- 以太网( windows, 中文系统 用网线联网的情况)
                -- Ethernet 5 (windows, 英文系统 ipconfig 会显示 Ethernet adapter Ethernet 5)

            }
        }, tlsout, websocket_out
        }
    }

    --[[

        演示 inbound 是 ip + 自动全局路由, outbound 是 tcp+tls+ws
        这就做出了一个简单的"VPN". 注意, 这种情况不可通过 tcp/udp 目标分流, 因为传递的直接是ip, 且未经任何探查和修改
        同时为了保证dns 不被污染, 要在 dns_list 中指定一个 好的dns

        注意, 这种简单的ip relay 还算不上是真正的VPN, 因为同一时间只支持一个设备连到服务端. 想达到真正VPN的效果
        需要真正的VPN协议

    --]]

}

---[[
local config_17_tcp_ip_stack = {

    inbounds = {
        listen1 = { {
            type = "BindDialer",
            bind_addr = "ip://10.0.0.1:24#utun321",

            in_auto_route = {
                tun_dev_name = "utun321",
                tun_gateway = "10.0.0.1",
                router_ip = "192.168.0.1",
                original_dev_name = "en0", -- "以太网"
                dns_list = { "114.114.114.114" }

            }
        }, "Stack" },
    },
    --outbounds = { dial1 = out_stdio_show_bytes_chain }
    outbounds = {
        dial1 = opt_direct_chain
    }

    -- outbounds = {
    --     dial1 = { {
    --         type = "OptDialer",
    --         dial_addr = "tcp://192.168.0.204:10801",
    --         sockopt = {
    --             bind_to_device = "en0"     -- "以太网"
    --         }

    --     }, tlsout, trojan_out }
    -- }
}
--]]

-- Recorder 用于记录流量并写入单独的日志文件
local config_18_recorder = {
    inbounds = {
        listen1 = { listen_10800, {
            type = "Recorder",
            label = "socks5",
            output_file_extension = "Json", --"Cbor"
            output_format = "Ruci",         --"Ruci", "Har"
            record_mode = "Info",
            output_dir = "record_dir",
            -- piece_truncate_option = "NoTruncate",
            -- session_truncate_option = "NoTruncate",
        }, { type = "Socks5Http" },
        },
    },
    outbounds = {
        dial1 = { direct,
            {
                type = "Recorder",
                label = "direct",
                output_file_extension = "Json",
                output_format = "Ruci",
                record_mode = "Info",
                output_dir = "record_dir",

                -- piece_truncate_option = "NoTruncate",
                -- session_truncate_option = "NoTruncate",

            }
        }
    }

}

local function get_recorder(label)
    return {
        type = "Recorder",
        label = label,
        serialize_format = "cbor",
        -- session_truncate = 2000, -- 就算设了 full_record = true 也是会默认有 truncate的，除非设了 no_truncate = true
        no_truncate = true,

    }
end

local function random_host()
    local hosts = {
        "www.baidu.com",
        "www.bilibili.com",
        -- "www.qq.com",
    }
    return hosts[math.random(1, #hosts)]
end

local config_19_recorder_trojans = {
    tag_route = { { "listen_socks5", "dial_trojans" }, { "listen_trojans", "dial_direct" } },
    inbounds = {
        listen_socks5 = {
            listen_10800,
            get_recorder("socks5"),
            {
                type = "Socks5Http",
            }
        },
        listen_trojans = { {
            type = "Listener", listen_addr = "0.0.0.0:10801"
        },
            get_recorder("trojans"),
            {
                type = "TLS",
                cert = "test.crt",
                key = "test.key",
                alpn = { "h2", "http/1.1" }


            },
            {
                type = "Trojan",
                password = "mypassword"

            } },
    },
    outbounds = {
        dial_direct = { direct, get_recorder("direct"), },
        dial_trojans = {
            {
                type = "BindDialer",
                dial_addr = "tcp://127.0.0.1:10801"

            },

            {
                type = "NativeTLS",
                --"TLS" = {
                host = random_host(), --"www.1234.com",
                insecure = true,
                alpn = { "http/1.1" }


            },
            get_recorder("trojan"),
            trojan_out

        }
    }

}

-- steganography protocol example 1
local config_20_spe1 = {
    inbounds = { listen1 = listen_socks5http },
    outbounds = {
        dial1 = { dial, { type = "SPE1", qa = { { "q1", "a1" }, { "q2", "a2" } } }, trojan_out }
    }
}

local config_21_lua_example1 = {
    inbounds = {
        listen1 = listen_socks5http,
    },
    outbounds = {
        dial1 = { dial, tlsout, trojan_out, { type = "Lua", file_name = "lua_protocol_e1.lua", handshake_function = "Handshake2" } }
    }
}

local config_22_lua_example2 = {
    inbounds = {
        listen_socks5http = "listen1"
    },
    outbounds = {
        dial1 = { dial, tlsout, trojan_out, { type = "Lua", file_name = "lua_protocol_e2_mathadd.lua", handshake_function = "Handshake" } }
    }
}


--[[
local config_23_tcp_ip_stack_lwip = {

    inbounds = {
        listen1 = {
            {
                BindDialer = {
                    bind_addr = "ip://10.0.0.1:24#utun321",

                    in_auto_route = {
                        tun_dev_name = "utun321",
                        tun_gateway = "10.0.0.1",
                        router_ip = "192.168.0.1",
                        original_dev_name = "en0",
                        dns_list = { "114.114.114.114" }
                    }
                }
            },
            "StackLwip" },
    },

    outbounds = {
        dial1 = { {
            OptDialer = {
                dial_addr = "tcp://192.168.0.10:10801",
                sockopt = {
                    bind_to_device = "en0"
                }
            }
        }, tlsout, trojan_out }
    }
}
--]]

local config_24_chain_mitm = {
    inbounds = {
        listen1 = {
            listen_10800,
            { type = "Socks5Http" },
            {
                type = "MITM",
                cert = "test_ca_cert.pem",
                key = "test_ca_key.pem",
                alpn = { "h2", "http/1.1" }

            },
        },
    },
    outbounds = {
        dial1 = {
            { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
            {
                type = "NativeTLS",
                host = "www.google.com",
                insecure = true,
                alpn = { "h2", "http/1.1" }

            },
            { type = "Trojan",     password = "mypassword" }

        }
    }
}

local config_25_recorder_mitm = {
    inbounds = {
        listen1 = { listen_10800,
            { type = "Socks5Http" },
            {
                type = "MITM",
                cert = "test_ca_cert.pem",
                key = "test_ca_key.pem",
                alpn = { "h2", "http/1.1" }

            },

            -- 一般 一个响应大概在100多毫秒后到达，此回包 的 timestamp 典型值可以为 131876208 (nano seconds)
            -- h2 的最后一个包 很多情形下为 17 字节 (goaway包)
            -- 本端关闭时还会有一个 0包 表示 EOF
            -- 如此， 一个典型的 h2 请求+响应 实际上 一般是由 四个 包构成

            {
                type = "Recorder",
                label = "mitm",
                output_file_extension = "Json", --"Cbor"
                output_format = "Ruci",         --"Ruci", "Har"
                record_mode = "Info",
                output_dir = "record_dir",
                prettify = true,

            },
        },
    },
    outbounds = {
        dial1 = { {
            type = "Direct",
            leak_target_addr = true -- 注意这里要设为 true, 这样才能把 目标地址进一步 传递到 TLS 层 (用于设置 SNI)

        },
            {
                type = "NativeTLS",
                alpn = { "h2", "http/1.1" },
                insecure = false

            }
        }
    }

}


local config_26_chain_mitm_embedder = {
    inbounds = {
        listen1 = {
            listen_10800,
            { type = "Socks5Http" },
            {
                type = "MITM",
                cert = "test_ca_cert.pem",
                key = "test_ca_key.pem",
                alpn = { "h2", "http/1.1" }

            },
        }
    },
    outbounds = {
        dial1 = {
            { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
            {
                type = "NativeTLS",
                host = "www.google.com",
                insecure = true,
                alpn = { "h2", "http/1.1" }

            },
            { type = "Embedder",   file_name = "record_dir1/1-2_mitm_ruci_info.json" },
            { type = "Trojan",     password = "mypassword",                          do_not_use_early_data = false }

        }
    }
}

local config_27_embedder = {
    inbounds = {
        listen1 = {
            listen_10800,
            { type = "Socks5Http" },
        }
    },
    outbounds = {
        dial1 = {
            { type = "BindDialer", dial_addr = "tcp://127.0.0.1:10801" },
            {
                type = "NativeTLS",
                host = "www.google.com",
                insecure = true,
                alpn = { "h2", "http/1.1" }

            },
            { type = "Embedder",   file_name = "test_mitm_ruci_info.json" },
            { type = "Trojan",     password = "mypassword",               do_not_use_early_data = false }

        }
    }
}

Config = config_27_embedder

-- local str = Load_file("test.crt") -- load file from the default file provider from ruci ( from either tar or folder)
-- print("content of crt is:", str)

---[[

-- 完全动态链的基本演示

-- 完全动态链不使用 固定的列表 来预定义任何Map, 它只给出一个函数
-- generator, generator 根据参数内容来动态生成 [Map], 如果不想
-- 重复生成以前生成过的Map, 则可以返回一个已创建过的Map (参见其它包含 Infinite 的配置文件中的示例)

-- 完全动态链需要在 ruci-cmd 运行时 加 --infinite 来启用

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
                        type = "Listener", listen_addr = "0.0.0.0:10800"
                    },
                    new_thread_fn = function(cid, state_index, data)
                        -- print("lua: cid",inspect(cid))
                        -- table.insert(my_cid_record,cid)
                        -- print("lua: cid cache",inspect(my_cid_record))

                        local new_cid, newi, new_data = coroutine.yield(1, {
                            type = "Socks5Http"
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
                return 0, direct
            else
                return -1, {}
            end
        end
    } }

}

-- ]]
