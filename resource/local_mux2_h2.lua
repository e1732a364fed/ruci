-- 演示了 用完全动态链实现 h2 mux outbound 的配置
-- 关注 outbounds 的 generator 部分, 它实现了 max_num 条 主h2连接的多路复用
max_num = 12

dial_config = {
    Dialer = "tcp://0.0.0.0:10801"
}

tlsout_config = {
    TLS = {
        host = "www.1234.com",
        insecure = true,
        alpn = {"h2"}

    }
}
trojan_out_config = {
    Trojan = "mypassword"
}

h2_out_config = {
    H2Mux = {
        is_grpc = true,
        http_config = {
            authority = "myhost",
            path = "/service1/Tun"
        }
    }
}

h2_single_out_config = {
    H2Single = {
        is_grpc = true,
        http_config = {
            authority = "myhost",
            path = "/service1/Tun"
        }
    }
}

h2_out_pool = {}
-- local inspect = require("inspect")
infinite = {

    inbounds = {{
        tag = "listen1",

        generator = function(cid, state_index, data)
            if state_index == -1 then
                return 0, {
                    stream_generator = {
                        Listener = "0.0.0.0:10800"
                    },
                    new_thread_fn = function(cid, state_index, data)

                        if socks5_in == nil then
                            socks5_in = create_in_mapper {
                                Socks5 = {}
                            }
                        end

                        local new_cid, newi, new_data = coroutine.yield(1, socks5_in:clone())
                        return -1, {}
                    end
                }
            end
        end
    }},

    outbounds = {{
        tag = "dial1",
        generator = function(cid, state_index, data)
            if state_index == -1 then

                local pool_n = table.getn(h2_out_pool)
                -- print("-1 pooln",pool_n, max_num)

                if pool_n >= max_num then
                    local i = math.random(1, pool_n)

                    return 2, h2_out_pool[i]:clone()
                end

                if dial_out == nil then
                    dial_out = create_out_mapper(dial_config)
                end
                return 0, dial_out:clone()

            elseif state_index == 0 then

                if tlsout == nil then
                    tlsout = create_out_mapper(tlsout_config)
                end

                return 1, tlsout:clone()
            elseif state_index == 1 then

                --[[

                -- 错误写法. 状态为1即有了tcp-tls 拨号, 此时必需建立新
                -- h2连接. 不能用原连接, 也不能用新连接替换原连接, 这都会
                -- 导致悬垂连接 

                
                local pool_n = table.getn(h2_out_pool)

                --print("1 pooln",pool_n, max_num)

                if pool_n < max_num then
                    local h2_out = create_out_mapper(h2_out_config)

                    table.insert(h2_out_pool,h2_out)
                    return 2, h2_out:clone()
                end

                local i = math.random(1,pool_n)
                return 2, h2_out_pool[i]:clone()


                -- ]]

                local pool_n = table.getn(h2_out_pool)

                if pool_n < max_num then

                    local h2_out = create_out_mapper(h2_out_config)
                    table.insert(h2_out_pool, h2_out)
                    return 2, h2_out:clone()
                end

                return 2, create_out_mapper(h2_single_out_config)

            elseif state_index == 2 then
                if trojan_out == nil then
                    trojan_out = create_out_mapper(trojan_out_config)
                end

                return 3, trojan_out:clone()
            else
                return -1, {}
            end
        end
    }}

}
