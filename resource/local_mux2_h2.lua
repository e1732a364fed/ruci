-- 演示了 用完全动态链实现 h2 mux outbound 的配置
-- 关注 outbounds 的 generator 部分, 它实现了 max_num 条 主h2连接的多路复用
local max_num = 12

local dial_config = {
    BindDialer = {
        dial_addr = "tcp://0.0.0.0:10801"
    }
}

local tlsout_config = {
    TLS = {
        host = "www.1234.com",
        insecure = true,
        alpn = { "h2" }

    }
}
local trojan_out_config = {
    Trojan = "mypassword"
}

local h2_common_part = {
    is_grpc = true,
    http_config = {
        authority = "myhost",
        path = "/service1/Tun"
    }
}

local h2_out_config = {
    H2Mux = h2_common_part
}

local h2_single_out_config = {
    H2Single = h2_common_part
}

local h2_out_pool = {}

-- local inspect = require("inspect")
Infinite = {

    inbounds = { {
        tag = "listen1",

        generator = function(cid, state_index, data)
            if state_index == -1 then
                return 0, {
                    stream_generator = {
                        Listener = { listen_addr = "0.0.0.0:10800" }
                    },
                    new_thread_fn = function(cid2, state_index2, data2)
                        if Socks5_in == nil then
                            Socks5_in = Create_in_map {
                                Socks5 = {}
                            }
                        end

                        local new_cid, newi, new_data = coroutine.yield(1, Socks5_in:clone())
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
                local pool_n = #h2_out_pool
                -- print("-1 pooln",pool_n, max_num)

                if pool_n >= max_num then
                    local i = math.random(1, pool_n)

                    return 2, h2_out_pool[i]:clone()
                end

                if Dial_out == nil then
                    Dial_out = Create_out_map(dial_config)
                end
                return 0, Dial_out:clone()
            elseif state_index == 0 then
                if Tlsout == nil then
                    Tlsout = Create_out_map(tlsout_config)
                end

                return 1, Tlsout:clone()
            elseif state_index == 1 then
                --[[

                -- 错误写法. 状态为1即有了tcp-tls 拨号, 此时必需建立新
                -- h2连接. 不能用原连接, 也不能用新连接替换原连接, 这都会
                -- 导致悬垂连接


                local pool_n = #h2_out_pool

                --print("1 pooln",pool_n, max_num)

                if pool_n < max_num then
                    local h2_out = Create_out_map(h2_out_config)

                    table.insert(h2_out_pool,h2_out)
                    return 2, h2_out:clone()
                end

                local i = math.random(1,pool_n)
                return 2, h2_out_pool[i]:clone()


                -- ]]

                local pool_n = #h2_out_pool

                if pool_n < max_num then
                    local h2_out = Create_out_map(h2_out_config)
                    table.insert(h2_out_pool, h2_out)
                    return 2, h2_out:clone()
                end

                return 2, Create_out_map(h2_single_out_config)
            elseif state_index == 2 then
                if Trojan_out == nil then
                    Trojan_out = Create_out_map(trojan_out_config)
                end

                return 3, Trojan_out:clone()
            else
                return -1, {}
            end
        end
    } }

}
