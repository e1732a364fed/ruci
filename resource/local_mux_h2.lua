-- 演示了 用完全动态链实现 h2 mux outbound 的配置
-- 关注 outbounds 的 generator 部分, 它实现了单h2连接的多路复用
dial_config = {
    Dialer = {
        dial_addr = "tcp://0.0.0.0:10801"
    }
}

tlsout_config = {
    --NativeTLS = {
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

                if h2_out ~= nil then
                    return 2, h2_out:clone()
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
                if h2_out == nil then
                    h2_out = create_out_mapper(h2_out_config)
                end

                return 2, h2_out:clone()
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
