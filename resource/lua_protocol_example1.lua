function Handshake1(cid, behavior, addr, firstbuff, conn)
    -- print("lua Handshake1 called")
    return conn, addr, firstbuff
end

function Handshake2(cid, behavior, addr, firstbuff, conn)
    -- print("lua Handshake2 called")

    TheConn = conn

    Cid = cid

    Behavior = behavior

    return { Read2, Write2, Close2, Flush2 }, addr, firstbuff
end

function Read2(buf, cx)
    -- print("lua read2 called")
    local x = TheConn:poll_read(cx, buf)

    if x:is_pending() then
        return -1
    elseif x:is_err() then
        return -2
    else
        return 0
    end
end

function Write2(str, cx)
    -- print("lua write2 called", str:len())
    local x = TheConn:poll_write(cx, str)

    if x:is_pending() then
        return -1
    elseif x:is_err() then
        return -2
    else
        local n = x:get_n()
        -- print("lua write2 finish", n)

        return n
    end
end

function Close2(cx)
    -- print("close2 called")
    local x = TheConn:poll_close(cx)

    if x:is_pending() then
        return -1
    elseif x:is_err() then
        return -2
    else
        return 0
    end
end

function Flush2(cx)
    -- print("flush2 called")

    local x = TheConn:poll_flush(cx)

    if x:is_pending() then
        return -1
    elseif x:is_err() then
        return -2
    else
        return 0
    end
end
