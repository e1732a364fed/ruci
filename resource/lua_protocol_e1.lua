local inspect = require("inspect")

function Handshake1(cid, behavior, addr, firstbuff, conn)
    -- print("lua Handshake1 called")
    return conn, addr, firstbuff
end

function Handshake2(cid, behavior, addr, firstbuff, conn)
    -- print("lua Handshake2 called")

    TheConn = conn

    Cid = cid

    Behavior = behavior

    -- 下面演示创建一个 read_buf 并向其中写内容

    local b = Create_read_buf(10)
    local fl = b:filled_len()
    print("fl", fl)
    b:put_slice("00123")
    fl = b:filled_len()
    print("fl", fl)
    local s = b:filled_content(fl)
    print("written head ", inspect(s))
    b:drop() --调用完 Create_read_buf 后，需要 调用 drop 来释放内存

    return { Read, Write, Close, Flush }, addr, firstbuff
end

-- 演示读取原流，并查看其头部
function Read(cx, buf)
    -- print("lua read2 called")
    local result = TheConn:poll_read(cx, buf)

    if result:is_pending() then
        return -1
    elseif result:is_err() then
        return -2
    else
        local rb = Wrap_read_buf(buf) -- 用 Wrap_read_buf 将 buf 转为 lua 可调用的 版本 (未转时仅能作 poll_read 的参数)

        local n = rb:filled_len()
        print("lua read2 got", n, Cid)

        if n > 10 then
            n = 10
        end

        local s = rb:filled_content(n)
        print("read head ", inspect(s:sub(1, 1))) --获取第一个字节的值 并打印出来

        return 0
    end
end

-- 演示按原流写入
function Write(cx, str)
    -- print("lua write2 called", str:len())
    local result = TheConn:poll_write(cx, str)

    if result:is_pending() then
        return -1
    elseif result:is_err() then
        return -2
    else
        local n = result:get_n()
        -- print("lua write2 finish", n)

        return n
    end
end

function Close(cx)
    -- print("close2 called")
    local result = TheConn:poll_close(cx)

    if result:is_pending() then
        return -1
    elseif result:is_err() then
        return -2
    else
        return 0
    end
end

function Flush(cx)
    -- print("flush2 called")

    local result = TheConn:poll_flush(cx)

    if result:is_pending() then
        return -1
    elseif result:is_err() then
        return -2
    else
        return 0
    end
end
