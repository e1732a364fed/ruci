-- local inspect = require("inspect")

--模仿 ruci::map::math::Adder 的行为：在 client 端 写时+add_num , 在 server 端 读时 -add_num

local add_num = 1

-- 每创建一个连接，本文件中的代码都会被重复调用一遍，因此在 全局上调用的函数要上心使用
-- print(Load_file("test.crt"))

local function read(cx, buf)
    if Behavior == 1 then -- client
        local result = TheConn:poll_read(cx, buf)

        if result:is_pending() then
            return -1
        elseif result:is_err() then
            Debug_print("lua read got err: " .. result:get_err())

            return -2
        else
            return 0
        end
    elseif Behavior == 2 then -- server
        local b = Create_read_buf(1024)
        local result = TheConn:poll_read(cx, b:get_ptr())

        if result:is_pending() then
            b:drop()
            return -1
        elseif result:is_err() then
            b:drop()
            Debug_print("lua read got err: " .. result:get_err())

            return -2
        else
            local n = b:filled_len()

            local s = b:filled_content(n)

            local byteArray = { string.byte(s, 1, #s) }
            for i = 1, #byteArray do
                local x = byteArray[i] - add_num
                byteArray[i] = x % 256
            end

            local new_s = string.char(table.unpack(byteArray))

            b:drop()

            local rb = Wrap_read_buf(buf)
            rb:put_slice(new_s)


            return 0
        end
    end
end

local function write(cx, s)
    if Behavior == 1 then -- client
        local byteArray = { string.byte(s, 1, #s) }
        for i = 1, #byteArray do
            local x = byteArray[i] + add_num
            byteArray[i] = x % 256
        end

        s = string.char(table.unpack(byteArray))
    end

    local result = TheConn:poll_write(cx, s)

    if result:is_pending() then
        return -1
    elseif result:is_err() then
        Debug_print("lua write got err: " .. result:get_err())

        return -2
    else
        local n = result:get_n()
        return n
    end
end

local function close(cx)
    local result = TheConn:poll_close(cx)

    if result:is_pending() then
        return -1
    elseif result:is_err() then
        Debug_print("lua close got err: " .. result:get_err())

        return -2
    else
        return 0
    end
end

local function flush(cx)
    local result = TheConn:poll_flush(cx)

    if result:is_pending() then
        return -1
    elseif result:is_err() then
        Debug_print("lua flush got err: " .. result:get_err())
        return -2
    else
        return 0
    end
end



function Handshake(cid, behavior, addr, firstbuff, conn)
    TheConn = conn
    Behavior = behavior
    return { read, write, close, flush }, addr, firstbuff
end
