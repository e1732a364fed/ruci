# lua用户自定义协议

ruci 中提供 lua用户自定义协议方式来 大大提高使用的灵活性。

方法是，用户首先在配置文件中指定 链中的一个Map为 Lua:

```lua
local config_21_lua_example1 = {
    inbounds = {
        listen1 = listen_socks5http,
    },
    outbounds = {
        dial1 = { dial, tlsout, trojan_out, { type = "Lua" ,file_name = "lua_protocol_e1.lua", handshake_function = "Handshake2"} }
    }
}
```

里面指定了 具体实现协议的 `lua_protocol_e1.lua` 文件 以及里面的 `Handshake2`函数 作为 协议的握手函数

lua_protocol_e1.lua：

```lua
function Handshake(cid, behavior, addr, firstbuff, conn)
    return conn, addr, firstbuff
end
```

cid 为字符串， behavior 为1 表示 client, 为 2 表示 server,
addr 表示 代理要连接的目标地址。
firstbuff 为数据的首包。
conn 为链中上一个Map 的连接。

上面函数的具体实现就是将内容按上一个Map的原样返回

如果要返回一个自定义的新Map，则可以返回一个 包含读、写、关、冲 四个函数的table

```lua
function Handshake(cid, behavior, addr, firstbuff, conn)
    Cid=cid
    TheConn = conn
    Behavior = behavior
     return { Read, Write, Close, Flush }, addr, firstbuff
end
```

上面函数把 cid, behavior, conn 保存到了全局变量中，这样就可以在 四函数中访问这些值了

其中， conn有 poll_read, poll_write, poll_close, poll_flush 四个方法。

它们都接收一个 cx 作为参数。这里不用管cx是什么，只要记住原样传递即可。

## Read

Read函数除了 cx外还有一个 buf 变量。
buf 变量可以作为 conn:poll_read的参数，也可以在 Wrap_read_buf(buf) 后变为一个
lua可以访问的变量类型，其有如下方法：

```lua
put_slice
filled_len
filled_content
```

这种类型的变量也可以用 `local b = Create_read_buf(1024)` 创建。

而如果要将 lua 可以访问的buf作为 poll_read的参数，则要加一个 `get_ptr`:

    conn:poll_read(cx, b:get_ptr())

而使用完 lua的buf后，要调用 `b:drop()` 来释放内存。


示例：


```lua
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
```

## Write

```lua
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
```

## Close

```lua
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
```

## Flush

```lua
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

```

## 其它

ruci还在lua中注册了 `Debug_print`,`Info_print`,`Warn_print` 函数，可以用于向日志打印自定义输出（以debug,info,warn 级别)

还有 Load_file 函数，可以用它加载 tar 中的文件。（只在 静态链中有效）
