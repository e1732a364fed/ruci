-- 演示了 用完全动态链实现 h2 mux outbound 的配置
-- 关注 outbounds 的 generator 部分, 它实现了单h2连接的多路复用
local dial_config = {
    BindDialer = {
        dial_addr = "tcp://0.0.0.0:10801"
    }
}

local function random_host()
    local hosts = {
        "www.baidu.com",
        "www.bilibili.com",
        -- "www.qq.com",
    }
    return hosts[math.random(1, #hosts)]
end

local function gen_new_tlsout_config()
    return {
        NativeTLS = {
            --TLS = {
            host = random_host(), --"www.1234.com",
            insecure = true,
            alpn = { "h2" }

        }
    }
end

local trojan_out_config = { Trojan = { password = "mypassword" } }

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
                    return 3, H2_out:clone()
                end

                if Dial_out == nil then
                    Dial_out = Create_out_map(dial_config)
                end
                return 0, Dial_out:clone()
            elseif state_index == 0 then
                if Recorder == nil then
                    Recorder = Create_out_map({
                        Recorder = {
                            -- label = "h2_trojans",
                            -- label = "h2_socks5s",
                            label = "h2_https",
                            serialize_format = "cbor",
                            session_truncate = 2000,
                        }
                    })
                end

                return 1, Recorder:clone()
            elseif state_index == 1 then
                if Tlsout == nil then
                    Tlsout = Create_out_map(gen_new_tlsout_config())
                end

                return 2, Tlsout:clone()
            elseif state_index == 2 then
                if H2_out == nil then
                    H2_out = Create_out_map(h2_out_config)
                end

                return 3, H2_out:clone()
            elseif state_index == 3 then
                -- if Trojan_out == nil then
                --     Trojan_out = Create_out_map(trojan_out_config)
                -- end

                -- return 4, Trojan_out:clone()
                -- if Socks5_out == nil then
                --     Socks5_out = Create_out_map({
                --         Socks5 = {
                --         }
                --     })
                -- end

                -- return 4, Socks5_out:clone()

                if Http_out == nil then
                    Http_out = Create_out_map("Http")
                end

                return 4, Http_out:clone()
            else
                return -1, {}
            end
        end
    } }

}
