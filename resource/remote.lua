print("this is a lua remote config file")

-- lua 的好处有很多, 你可以定义很多变量

local tcp = {
    Listener = {
        listen_addr = "0.0.0.0:10801"
    }
}
local unix = {
    Listener = {
        listen_addr = "unix://file1"
    }
}

local opt_direct_chain = { {
    OptDirect = {
        sockopt = {
            so_mark = 255,
            bind_to_device = "enp0s1"
        },
        more_num_of_files = true, -- auto run system call to increase NOFILE to prevent Too many of files, root required
    }
} }

local socks5_chain = { tcp, {
    Socks5 = {}
} }
local http_chain = { tcp, {
    Http = {}
} }
local socks5http_chain = { tcp, {
    Socks5Http = {}
} }

local tls = {
    -- NativeTLS = {
    TLS = {
        cert = "test.crt",
        key = "test.key",
        alpn = { "h2", "http" }

    }
}

local trojan_in = {
    Trojan = {
        password = "mypassword"
    }
}

local trojan_chain = { tcp, trojan_in }
local trojans_chain = { tcp, tls, trojan_in }

local http_filter = {
    HttpFilter = {
        authority = "myhost",
        path = "/path1"
    }
}

local basic_ws = {
    WebSocket = {}
}

local ws = {
    WebSocket = {
        http_config = {
            authority = "myhost",
            path = "/path1"
        }
    }
}

-- use http_filter to support fallback.

-- if http_filter is used,
-- http_config field in WebSocket can be omitted.

local ws_trojans_chain = { tcp, tls, http_filter, basic_ws, trojan_in }

-- ws_trojans_chain = {tcp, tls, ws, trojan_in}

local in_h2_trojans_chain = { tcp, tls, {
    H2 = {
        is_grpc = true,
        http_config = {
            authority = "myhost",
            path = "/service1/Tun"
        }
    }
}, trojan_in }

local in_quic_chain = { {
    Quic = {
        key_path = "test2.key",
        cert_path = "test2.crt",
        listen_addr = "127.0.0.1:10801",
        alpn = { "h3" }
    }
}, trojan_in }

local dial = {
    BindDialer = {
        dial_addr = "tcp://0.0.0.0:10801"
    }
}

local dial_trojan = { dial, {
    Trojan = "mypassword"
} }

local out_stdio_chain = { {
    Stdio = {}
} }

local direct_out_chain = { "Direct" }

Config = {
    inbounds = { --  { chain = trojan_chain,  tag = "listen1"}
        { chain = trojans_chain, tag = "listen1" },
        -- { chain = ws_trojans_chain,  tag = "listen1"  }
        -- { chain = in_h2_trojans_chain, tag = "listen1" }
        -- { chain = in_quic_chain, tag = "listen1" }
        -- { chain = socks5http_chain, tag = "listen1"} ,
        -- { chain =  { unix,tls, trojan_in }, tag = "listen1"} ,
        --[[
        {
            chain = {{
                BindDialer = {
                    bind_addr = "udp://127.0.0.1:20800"
                }
            }, "Echo"},
            tag = "udp_echo"

        }
        -- ]]
    },

    --[[
    -- 一般情况下 的 outbound 配置

    outbounds = { {
        tag = "dial1",
        chain = direct_out_chain
    }, {
        tag = "fallback_d",
        chain = { {
            BindDialer = {
                dial_addr = "tcp://0.0.0.0:80"
            }
        } }
    } },
    -- ]]

    ---[[
        -- 对应 local.lua 使用 tproxy 的 outbound 配置
        -- 如果 用 tproxy 时 direct 不用 opt_direct 设置 somark, 将造成无限回环, 无法联网

        -- 不过这是 本示例中 单机自连的做法. 如果实现 remote.lua 部署在远程服务器上, 是不需要 OptDirect 的

        outbounds = {{
            tag = "dial1",
            chain = opt_direct_chain
        }},
    --]]

    -- outbounds = { { tag="dial1", chain = out_stdio_chain  } }, --以命令行为出口

    fallback_route = { { "listen1", "fallback_d" } }

}
