-- 演示了 用完全动态链实现 h2 mux outbound 的配置
-- 关注 outbounds 的 generator 部分, 它实现了单h2连接的多路复用
dial = {
    Dialer = "tcp://0.0.0.0:10801"
}

tlsout = {
    TLS = {
        host = "www.1234.com",
        insecure = true
    }
}
trojan_out = {
    Trojan = "mypassword"
}

socks5_out = {
    Socks5 = {}
}

h2_out = {
    H2Mux = {
        is_grpc = true,
        http_config = {
            host = "myhost",
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

                        if socks5_in_mapper == nil then
                            socks5_in_mapper = create_in_mapper(socks5_out)
                        end

                        local new_cid, newi, new_data = coroutine.yield(1, socks5_in_mapper:clone())
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

                if h2_mapper ~= nil then
                    return 2, h2_mapper:clone()
                end

                if dial_mapper == nil then
                    dial_mapper = create_out_mapper(dial)
                end
                return 0, dial_mapper:clone()

            elseif state_index == 0 then

                if tlsout_mapper == nil then
                    tlsout_mapper = create_out_mapper(tlsout)
                end

                return 1, tlsout_mapper:clone()
            elseif state_index == 1 then
                if h2_mapper == nil then
                    h2_mapper = create_out_mapper(h2_out)
                end

                return 2, h2_mapper:clone()
            elseif state_index == 2 then
                if trojan_mapper == nil then
                    trojan_mapper = create_out_mapper(trojan_out)
                end

                return 3, trojan_mapper:clone()
            else
                return -1, {}
            end
        end
    }}

}
