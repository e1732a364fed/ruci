
开发相关：参考 rucimp/src/modes/chain/config/mod.rs

开发相关：因为 代码实现方式不同，有些功能相近的 Map的Config是独立的

在 Map说明的 首部标有 in, out 或 in/out 字样，表明可用于 InMapConfig 还是 OutMapConfig

没有任何示例的Map 意为着其写法为 `"Name"`, 不 外加大括号，如 `"Echo"` , `"Blackhole"`

其它的配置均要外加 大括号，如 `Direct = {}` 意味着要 写为 `{Direct = {}}`  才算一个完整的 table 



# 入口、出口 Map

## Blackhole
out

## Direct
out

```lua
Direct = {}
```

可选项为 dns_client 


```lua
Direct = {
    dns_client = {
        --...
    }--optional
}
```
见[DnsClient](#dnsclient)


### OptDirect
in

OptDirect 的出现是 为了给 Direct 添加 sockopt 选项。使用 tproxy 要用该Map


```lua
OptDirect = {
    sockopt= {
        --...
    },
    more_num_of_files= false, -- 可选
    dns_client = {}, -- 可选
}
```

见[SockOpt](#sockopt)

见[DnsClient](#dnsclient)

more_num_of_files 为 true时，在 linux 上，程序将自动调整 系统设置,
防止 出现 num_of_files 不够的问题 ( 在 tproxy 等情况下尤为严重)



## BindDialer
in

BindDialer 是 一个 既可以 Bind 又可以 Dial 的配置

Bind 用于 udp 和 ip, dial 则用于 udp,tcp,uds(unix domain socket)

BindDialer 中所有项都是可选的，但 bind_addr 或 dial_addr 中至少有一个要设置

    对于 ip, bind_addr 须提供, 否则将报错

    对于 tcp/udp, 如果 bind_addr 不提供, 将采用 随机端口

    对于 uds, bind_addr 无意义

    对于 ip, dial_addr 无意义

    对于 tcp/uds, dial_addr 须提供，否则将报错


```lua
 BindDialer = {
    bind_addr = "",
    dial_addr = "",

    dns_client= {..} --optional

    --#[cfg(feature = "tun")]
    in_auto_route= {..}, 

    --#[cfg(feature = "tun")]
    out_auto_route = {..}, 

    ext= {..}, --optional
}
```

见[DnsClient](#dnsclient)

见[Ext](#ext)


### OptDialer
in

```lua
  OptDialer = {
    dial_addr= "",
    sockopt= {}, --optional
    dns_client = {}, --optional
}
```

见[DnsClient](#dnsclient)

见[SockOpt](#sockopt)

## Listener
in

```lua
Listener = {
    listen_addr ="",
    ext={},--optional
}
```
见[Ext](#ext)

## TcpOptListener
in

```lua
TcpOptListener = {
    listen_addr ="",
    sockopt={},
    ext={},--optional
}
```

见[SockOpt](#sockopt)

见[Ext](#ext)

## Stdio

in/out

```lua
 StdioConfig = {
    write_mode = "Bytes", --UTF8
    ext={},--optional
}
```

默认的 write_mode 为 UTF8, 可以用 Bytes 模式来观察16进制数据

见[Ext](#ext)


## Fileio
in/out

```lua
  Fileio = {
    i="",
    o="",
    sleep_interval=1, --optional, 正整数
    bytes_per_turn=100, --optional, 正整数
    ext={}, --optional
}
```

见[Ext](#ext)

## Tproxy 
in

linux only

### TproxyUdpListener

```lua
TproxyUdpListener = {
    listen_addr="",
    sockopt={},
    ext={}, --optional
}
```

见[SockOpt](#sockopt)

见[Ext](#ext)

### TproxyTcpResolver

rucimp/src/map/tproxy/route/mod.rs

```lua
TproxyTcpResolver= {
    -- tproxy 监听的端口, 默认为 12345
    port=12345, --  正整数
    route_ipv6= false,
    proxy_local_udp_53=false,

    --局域网段, 默认为 192.168.0.0/16
    local_net4 = "192.168.0.0/16",
    auto_route = true,
    auto_route_tcp=false,
}
```

所有项都是可选的

auto_route 为 true 时， 若 auto_route_tcp 也为 true, 则 自动路由过程 只会为 tcp 设置路由,
udp 将不被路由到tproxy中.

## Stack
in

## 

# 网络协议 Map
in/out

## 简单代理协议 Socks5,Http,Socks5Http

```lua
Socks5Http = {}
Socks5 = {}
Http = {}
```

可选用户密码组合, 内容均为可选

```lua
Socks5Http = { -- Socks5, Http
    userpass: "username1 password1",
    more: { "username2 password2", "username3 password3"},
}
```

字符串中 按whitespace 分割


## Trojan

```lua
Trojan = {
    password: "password1",
    more: { "password2", "password3"},
}
```

同上。password 以明文书写。


## TLS
in/out

in:

```lua
 Tls = {
    cert="c.crt",
    key="k.key",
    alpn = { "h2", "h3"},--optional
}
```

out:

```lua
 Tls = {
    host="www.myhost.com",
    insecure=false,
    alpn = { "h2", "h3"},--optional
}
```

如果任意一方的alpn 没给出, 则连接都通过；如果两方 alph 都给出, 则只有匹配了才通过


## NativeTLS
同上

## http2

服务端用 H2, 客户端用 H2Single 或 H2Mux，一般用 H2Mux 以使用 多路复用

grpc 也是在 http2 配置中设置


### H2

目前 h2 的三种Map 的 Config 格式 是一样的

in

```lua
H2 ={
    is_grpc=false,
    http_config={},
}
```
见 [HttpCommonConfig](#httpcommonconfig)

### H2Single

out

```lua
H2Single ={
    is_grpc=false,
    http_config={},
},
```

见 [HttpCommonConfig](#httpcommonconfig)

### H2Mux

out

```lua
H2Mux ={
    is_grpc=false,
    http_config={},
},
```
见 [HttpCommonConfig](#httpcommonconfig)

## WebSocket
in/out


in:

```lua
WebSocket={
    http_config = {
        --... optional
    }
}
```
见 [HttpCommonConfig](#httpcommonconfig)


out:

```lua
WebSocket={
    --... optional
}
```
见 [HttpCommonConfig](#httpcommonconfig)


## Quic

in/out


in:

quic 的 监听端 是直接接管 udp 层的, listen_addr 在这里指定, 而不额外用 Listener


```lua
Quic= {
    key_path="",
    cert_path="",
    listen_addr="",
    alpn = { "h2", "h3"},--optional
}
```

out:

```lua
Quic= {
    server_addr="",
    server_name="",
    cert_path="",--optional
    alpn = { "h2", "h3"},--optional
    is_insecure=true,--optional
}
```


# 辅助 Map

## Echo
in/out


## Adder
in/out

```lua
Adder=3
```

## Counter

in/out


## HttpFilter

in

```lua
HttpFilter={
    --...
}
```

见 [HttpCommonConfig](#httpcommonconfig)

# 子块

## DnsClient

```lua
dns_client = {
    dns_server_list = { { "127.0.0.1:20800", "udp" } }, -- 8.8.8.8:53
    ip_strategy = "Ipv4Only",
    static_pairs = {
        ['www.baidu.com'] = "103.235.47.188"
    }
}

```
Direct,OptDirect,BindDialer,OptDialer 都可加此块。

## SockOpt

```lua
sockopt = {
    tproxy = true,
    so_mark = 255,
    bind_to_device = "en0",
}
```

三项都是可选的

tproxy 为 true时表示 开启 tproxy 功能

so_mark 从0 到 255.

bind_to_device 的一些可能的值：

-- enp0s1(linux 的一般情况)
-- en0  (macos 的情况)
-- WLAN( windows, 用wifi联网的情况)

## Ext

Listener,TcpOptListener, BindDialer, Stdio, Fileio 都能如此配置 ext

```lua
ext = {
    fixed_target_addr = "udp://8.8.8.8:53",
    pre_defined_early_data = "abcde"
}
```

ext 是 extension 的缩写, 是指定一些额外配置, 内部的项都是可选的, ext 本身也是可选的

fixed_target_addr 一旦设置，意味着 该Map的实际的 代理目标 是指定的值。

常见用例是转发 udp 流量，比如为自己的 出口设置不同的 dns：

listen 一个 本地的 udp 端口 (a), 指定 ext.fixed_target_addr (b), 其为实际想要的dns服务器
然后将 这个listen 所在的链  route 到一个 与远程服务器连接 的 链1上。

之后在自己的 Direct 的 dns_client 中的 dns_server_list 中添加 a, 这样就可以
将所有direct 中用到的dns解析都实际 通过 链1 发送到 服务端，服务端 再将 dns信息发到 fixed_target_addr

这就是 一些代理中的 "dokodemo" 的逻辑.

## httpCommonConfig


```lua
 {
    method = "GET",--optional
    scheme = "https",--optional
    authority = "www.myhost.com",
    path = "/ruci_jiandan",
    headers= { ["header1"] = "value1", ["header2"] = "value2", },--optional
}
```

# 接下来

现在再读 resource/local.lua 就会轻松很多了。

学点难的？
[Infinite](./lua/infinite.md)
