local inspect = require("inspect")

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
                        local new_cid, newi, new_data = coroutine.yield(1, {
                            Socks5 = {}
                        })
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
                return 0, dial
            elseif this_index == 0 then
                return 1, tlsout
            elseif this_index == 1 then
                return 2, "H2Single"
            elseif this_index == 2 then
                return 3, trojan_out
            else
                return -1, {}
            end
        end
    }}

}
