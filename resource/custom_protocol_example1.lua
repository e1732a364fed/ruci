function Handshake1(cid, behavior, addr, firstbuff, conn)
    -- print("lua Handshake1 called")
    return conn, addr, firstbuff
end

function Handshake2(cid, behavior, addr, firstbuff, conn)
    -- print("lua Handshake2 called")

    _G["TheConn"] = conn


    return { "Read2", "Write2", "Close2", "Flush2" }, addr, firstbuff
end

function Read2(maxlen)
    print("lua read2 called", maxlen)
    local data = TheConn:read(maxlen)
    print("lua read got ", data:len())
    return data
end

function Write2(str)
    print("lua write2 called", str:len())
    local n = TheConn:write(str)
    print("lua write2 finish", n)

    return n
end

function Close2()
    print("close2 called")
    return TheConn:close()
end

function Flush2()
    print("flush2 called")

    return TheConn:flush()
end
