-- 演示了 用完全动态链实现 h2 mux outbound 的配置
-- 关注 outbounds 的 generator 部分, 它实现了单h2连接的多路复用
local dial_config = {
    BindDialer = {
        dial_addr = "tcp://0.0.0.0:10801"
    }
}

local tlsout_config = {
    --NativeTLS = {
    TLS = {
        host = "www.1234.com",
        insecure = true,
        alpn = { "h2" }

    }
}
local trojan_out_config = {
    Trojan = "mypassword"
}

local h2_out_config = {
    H2Mux = {
        is_grpc = true,
        http_config = {
            authority = "myhost",
            path = "/service1/Tun"
        }
    }
}

Infinite = {

    inbounds = { {
        tag = "listen1",

        generator = function(cid, state_index, data)
            if state_index == -1 then
                return 0, {
                    stream_generator = {
                        Listener = { listen_addr = "0.0.0.0:10800" }
                    },
                    new_thread_fn = function(cid, state_index, data)
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
                if H2_out ~= nil then
                    return 2, H2_out:clone()
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
                if H2_out == nil then
                    H2_out = Create_out_map(h2_out_config)
                end

                return 2, H2_out:clone()
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
