-- 本文件演示了 在客户端 随机连接 3个服务器 中的一个 的 负载均衡，里面使用三个不同的 tcp, tls,  trojan 配置 的情况

local dial_list = {
    "tcp://1.0.0.0:10801",
    "tcp://2.0.0.0:10801",
    "tcp://3.0.0.0:10801"
}

local function gen_rand_server_dial()
    local num = math.random(1, #dial_list)
    return num, {
        BindDialer = {
            dial_addr = dial_list[num]
        }
    }
end

local tls_list = {
    {
        TLS = {
            host = "www.server1.com",
            insecure = true,

        }
    },
    {
        TLS = {
            host = "www.server2.com",
            insecure = true,

        }
    },
    {
        TLS = {
            host = "www.server3.com",
            insecure = true,
        }
    }
}

local trojan_list = {
    {
        Trojan = "mypassword1"
    },
    {
        Trojan = "mypassword2"
    },
    {
        Trojan = "mypassword3"
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
                local idx, config = gen_rand_server_dial()
                return idx, Create_out_map(config)
            elseif state_index >= 1 and state_index <= #dial_list then
                return 10 * state_index, Create_out_map(tls_list[state_index])
            elseif state_index >= 10 and state_index <= 10 * #tls_list then
                return 100, Create_out_map(trojan_list[state_index / 10])
            else
                return -1, {}
            end
        end
    } }

}
