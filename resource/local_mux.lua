-- local inspect = require("inspect")

-- my_cid_record = {}
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

infinite = {

    inbounds = {{
        tag = "listen1",

        generator = function(cid, this_index, data)
            if this_index == -1 then
                return 0, {
                    stream_generator = {
                        Listener = "0.0.0.0:10800"
                    },
                    new_thread_fn = function(cid, this_index, data)

                        if socks5_in_mapper == nil then
                            socks5_in_mapper = create_in_mapper_func(socks5_out)
                        end

                        local new_cid, newi, new_data = coroutine.yield(1,socks5_in_mapper:clone() )
                        return -1, {}
                    end
                }
            end
        end
    }},

    outbounds = {{
        tag = "dial1",
        generator = function(cid, this_index, data)
            if this_index == -1 then
                if dial_mapper == nil then
                    print("lua creating dial_mapper")
                    dial_mapper = create_out_mapper_func(dial)
                end
                return 0, dial_mapper:clone()
            elseif this_index == 0 then
                if tlsout_mapper == nil then
                    tlsout_mapper = create_out_mapper_func(tlsout)
                end

                return 1, tlsout_mapper:clone()
            elseif this_index == 1 then
                if h2_mapper == nil then
                    h2_mapper = create_out_mapper_func("H2Single")
                end

                return 2, h2_mapper:clone()
            elseif this_index == 2 then
                if trojan_mapper == nil then
                    trojan_mapper = create_out_mapper_func(trojan_out)
                end

                return 3, trojan_mapper:clone()
            else
                return -1, {}
            end
        end
    }}

}
