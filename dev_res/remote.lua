print("this is a lua remote config file")

-- lua 的好处有很多, 你可以定义很多变量

local tcp = {
    type = "Listener",
    listen_addr = "0.0.0.0:10801"

}
local unix = {
    type = "Listener",
    listen_addr = "unix://file1"

}

local opt_direct_chain = { {
    type = "OptDirect",
    sockopt = {
        so_mark = 255,
        bind_to_device = "wlp3s0" --"enp0s1"
    },
    more_num_of_files = true,     -- auto run system call to increase NOFILE to prevent Too many of files, root required

} }

local socks5 = {
    type = "Socks5"
}
local socks5_chain = { tcp, socks5 }
local http_chain = { tcp, {
    type = "Http"
} }
local socks5http_chain = { tcp, {
    type = "Socks5Http"
} }

local tls = {
    -- NativeTLS = { --NativeTLS 要用 test2.crt 而不是 test.crt
    type = "TLS",
    cert = "test2.crt",
    key = "test2.key",
    alpn = { "h2", "http/1.1" },
    insecure = true

}

local trojan_in = {
    type = "Trojan",
    password = "mypassword"

}

local trojan_chain = { tcp, trojan_in }
local trojans_chain = { tcp, tls, trojan_in }

local embedder_in = {
    type = "Embedder",
    file_name = "test_mitm_ruci_info.json"

}

local http_filter = {
    type = "HttpFilter",
    authority = "myhost",
    path = "/path1"

}

local basic_ws = {
    type = "WebSocket"
}

local ws = {
    type = "WebSocket",
    http_config = {
        authority = "myhost",
        path = "/path1"
    }

}

-- use http_filter to support fallback.

-- if http_filter is used,
-- http_config field in WebSocket can be omitted.

local ws_trojans_chain = { tcp, tls, http_filter, basic_ws, trojan_in }

-- ws_trojans_chain = {tcp, tls, ws, trojan_in}

local h2 = {
    type = "H2",
    is_grpc = true,
    http_config = {
        authority = "myhost",
        path = "/service1/Tun"
    }

}

local in_h2_trojans_chain = { tcp, tls, h2, trojan_in }

local in_h2_socks5s_chain = { tcp, tls, h2, socks5 }

local in_h2_https_chain = { tcp, tls, h2, {
    type = "Http"
} }


local quic_in = {
    type = "Quic",
    key_path = "test2.key",
    cert_path = "test2.crt",
    listen_addr = "0.0.0.0:10801",
    alpn = { "h3" }

}

local in_quic_chain = { quic_in, trojan_in }

local dial = {
    type = "BindDialer",
    dial_addr = "tcp://0.0.0.0:10801"

}

local dial_trojan = { dial, trojan_in }

local out_stdio_chain = { {
    type = "Stdio"
} }

local out_stdio_show_bytes_chain = { {
    type = "Stdio",
    write_mode = "Bytes" -- 默认的 write_mode 为 UTF8, 可以用 Bytes 模式来观察16进制数据

} }

local spe1_in = { type = "SPE1", qa = { { "q1", "a1" }, { "q2", "a2" } } }
-- local spe1_in = { SPE1 = {} }

local lua_example1 = { tcp, tls, trojan_in, { type = "Lua", file_name = "lua_protocol_e1.lua", handshake_function = "Handshake2" } }
local lua_example2 = { tcp, tls, trojan_in, { type = "Lua", file_name = "lua_protocol_e2_mathadd.lua", handshake_function = "Handshake" } }

Config = {
    inbounds = { --  { chain = trojan_chain,  tag = "listen1"}
        -- { chain = trojans_chain, tag = "listen1" },
        { chain = { tcp, tls, embedder_in, trojan_in }, tag = "listen1" }
        -- { chain = ws_trojans_chain, tag = "listen1" }
        -- { chain = in_h2_trojans_chain, tag = "listen1" }
        -- { chain = in_h2_socks5s_chain, tag = "listen1" }
        -- { chain = in_h2_https_chain, tag = "listen1" }
        -- { chain = in_quic_chain, tag = "listen1" }
        -- { chain = socks5http_chain, tag = "listen1" },
        -- { chain = { unix, tls, trojan_in }, tag = "listen1" },
        -- { chain =  { tcp,tls, ws}, tag = "listen1"} ,
        --[[
        {
            chain = {{
                type = "BindDialer",
                    bind_addr = "udp://127.0.0.1:20800"

            },{ type = "Echo"}},
            tag = "udp_echo"

        }
        -- ]]
        -- { chain = { tcp, spe1_in, trojan_in }, tag = "listen1" }
        -- { chain = lua_example1, tag = "listen1" },
    },

    ---[[
    -- 一般情况下 的 outbound 配置

    outbounds = { {
        tag = "dial1",
        chain = { { type = "Direct" } }
    },

        ---[=[
        {
            tag = "fallback_d",
            chain = { {
                type = "BindDialer",
                dial_addr = "tcp://0.0.0.0:80"

            } }
        }
        --]=]
    },
    -- ]]

    --[[
    -- 对应 客户端使用 mitm 时，服务端的 outboud 配置。
    -- 注意 direct 后面要加上 TLS 来重新包装数据，否则隐私信息会明文传递在 服务器 与 目标地址 的网络链路上
    -- 而且这里的 TLS 最好使用的是 NativeTLS, 以增强真实性

    outbounds = { {
        tag = "dial1",
        chain = { {
            type = "Direct",
                leak_target_addr = true -- 注意这里要设为 true, 这样才能把 目标地址进一步 传递到 TLS 层 (用于设置 SNI)

        },
            {
                type = "TLS",
                    alpn = { "h2", "http/1.1" },
                    insecure = false

            }
        }
    }, {
        tag = "fallback_d",
        chain = { {
            type = "BindDialer",
                dial_addr = "tcp://0.0.0.0:4433" --mitm 的话，回落就是要到 https

        },
        }
    },
    },

    --]]


    --[[
    -- 对应 local.lua 使用 tproxy 的 outbound 配置
    -- 如果 用 tproxy 时 direct 不用 opt_direct 设置 somark, 将造成无限回环, 无法联网

    -- 不过这是 本示例中 单机自连的做法. 如果实现 remote.lua 部署在远程服务器上, 是不需要 OptDirect 的

    outbounds = { {
        tag = "dial1",
        chain = opt_direct_chain
    } },
    --]]

    --[[
    -- 对应 local.lua 使用 tun 的 outbound 配置.
    --  注意, 不像 tproxy, tun 示例不能本机自连测试

    outbounds = { {
        tag = "dial1",
        chain = {
            {
                type = "BindDialer",
                    bind_addr = "ip://10.0.0.2:24#utun321",

                    -- out_auto_route 会自动配置路由表使得 utun321 中的流量走 enp0s1.
                    -- 注意要确保开启了 ip_forward

                    -- out_auto_route 目前只支持 linux

                    out_auto_route = {
                        tun_dev_name = "utun321",
                        original_dev_name = "enp0s1", --wlp3s0
                        router_ip = "192.168.0.1",
                    }

            }
        }
    } },
    --]]


    -- outbounds = { { tag="dial1", chain = out_stdio_chain  } }, --以命令行为出口
    --outbounds = { { tag = "dial1", chain = out_stdio_show_bytes_chain } },

    fallback_route = { { "listen1", "fallback_d" } }

}
