
开发相关：参考 rucimp/src/modes/chain/config/mod.rs

开发相关：因为 代码实现方式不同，有些功能相近的 Map的Config是独立的

# 说明

在 Map说明的 首部标有 in, out 或 in/out 字样，表明可用于 InMapConfig 还是 OutMapConfig

配置的基本写法为 `{type = "Name"}`，内部还可能有其它项。

没有任何示例的Map 意为着其写法仅为 `{type = "Name"}`，如 `{ type = "Echo"}` , `{type = "Blackhole"}`,  `{type = "Direct"}`  


标有 ` --optional` 的项为可选项。

## `_addr` 的写法

一些项，如 bind_addr，dial_addr, listen_addr, fixed_target_addr 等，其写法都是相同的。如下每一行都是合法的示例

    fake.com:80
    [::1]:30800
    unix://file1
    udp://127.0.0.1:20800
    tcp://0.0.0.0:80
    ip://10.0.0.1:24#utun321

`{scheme}://`没给出时，默认使用 tcp. unix 表示 unix domain socket

方括号括起的表示 ipv6

ip 示例中的 24 表示 子网掩码的CIDR表示， utun321 为指定要创建的 tun网卡 名称


# 入口、出口 Map

## Blackhole
out

## Direct
out

```lua
type = "Direct"
```

可选项为 dns_client 


```lua
{
    type = "Direct",
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
{
    type = "OptDirect",
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
{
    type = "BindDialer",

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


in_auto_route 用于 tun

```lua
 in_auto_route = {
    tun_dev_name = "utun321",
    tun_gateway = "10.0.0.1",
    router_ip = "192.168.0.1",
    original_dev_name = "enp0s1", -- windows/macos 可不填 original_dev_name, linux 要填 original_dev_name
    --direct_list = { "192.168.0.204" }, -- 服务端的ip要直连
    dns_list = { "114.114.114.114" }
}

```

out_auto_route 只用于 ip转发 (从本地的tun 转发到 服务器上的 tun)

```lua
out_auto_route = {
    tun_dev_name = "utun321",
    original_dev_name = "enp0s1", --wlp3s0
    router_ip = "192.168.0.1",
}
```

ip 拨号是建立一个虚拟网卡，一般为 tun. 这个一般可以用于配合 tcp/ip stack (smoltcp/lwip) 进行全局路由使用，详情
见 local.lua 中的对应示例，以及 [这里](https://github.com/e1732a364fed/ruci/blob/tokio/doc/notes.md#tun模式的一些实测信息)


### OptDialer
in

```lua
{
    type = "OptDialer",
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
{
    type = "Listener",
    listen_addr ="",
    ext={},--optional
}
```
见[Ext](#ext)

## TcpOptListener
in

```lua
{
    type = "TcpOptListener",
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
{
    type = "Stdio",
    write_mode = "Bytes", --optional
    ext={},--optional
}
```

默认的 write_mode 为 `UTF8`, 可以用 `Bytes` 模式来观察16进制数据

见[Ext](#ext)


## Fileio
in/out

```lua
{
    type = "Fileio",
    i="",
    o="",
    sleep_interval=1, --optional, 正整数
    bytes_per_turn=100, --optional, 正整数
    ext={}, --optional
}
```

i 表示 输入文件名，o表示输出文件名。

i为自己预先定义好的文件，o则是一个给出的文件名，其内容只有通讯后才知道。

sleep_interval 和 bytes_per_turn 两项配置用于设置发送信息的速率。


见[Ext](#ext)

## Tproxy 
in

linux only

tproxy 的配置要复杂一些，要分 tcp 和 udp 两部分配置，自动路由的配置则是在 TproxyTcpResolver 中设置的。

```lua
local tproxy_tcp_listen = {
    type = "TcpOptListener",
    listen_addr = "0.0.0.0:12345",
    sockopt = {
        tproxy = true,
    }
}

local tproxy_listen_tcp_chain = {
    tproxy_tcp_listen, {
        type = "TproxyTcpResolver",
        port = 12345,
        --auto_route_tcp = true, -- only set route for tcp
        auto_route = true,         -- auto_route will set route for both tcp and udp at the appointed port

        route_ipv6 = true,         -- 如果为true, 则  也会 对 ipv6 网段执行 自动路由

        proxy_local_udp_53 = true, -- 如果为true, 则 udp 53 端口不会直连, 而是会流经 tproxy

        -- local_net4 = "192.168.0.0/16" -- 直连 ipv4 局域网段 不给出时, 默认即为 192.168.0.0/16
        
    }
}


local tproxy_udp_listen = {
    type = "TproxyUdpListener",
    listen_addr = "udp://0.0.0.0:12345",
    sockopt = {
        tproxy = true,
    }
    
}

local tproxy_listen_inbounds = { 
    listen_tproxy_tcp = tproxy_listen_tcp_chain,
    listen_tproxy_udp = { tproxy_udp_listen },
}

```



### TproxyUdpListener

```lua
{
    type = "TproxyUdpListener",
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
{
    type = "TproxyTcpResolver",
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

## StackSmoltcp/StackLwip
in

## 

# 网络协议 Map
in/out

## 简单代理协议 Socks5,Http,Socks5Http

```lua
type = "Socks5Http"
type = "Socks5"
type = "Http"
```

可选用户密码组合, 内容均为可选

```lua
{
    type = "Socks5", -- Http
    userpass: "username1 password1",
    more: { "username2 password2", "username3 password3"},
}
```

字符串中 按whitespace 分割


## Trojan

in:
```lua
{
    type = "Trojan", 
    password: "password1",
    more: { "password2", "password3"},
}
```

同上。password 以明文书写。

out:

```lua
 { type = "Trojan", password = "mypassword" }
```


## TLS
in/out

in:

```lua
{
    type = "TLS", 
    cert="c.crt",
    key="k.key",
    alpn = { "h2", "h3"},--optional
}
```

out:

```lua
{
    type = "TLS", 
    host="www.myhost.com",
    insecure=false,
    alpn = { "h2", "h3"},--optional
}
```

如果任意一方的alpn 没给出, 则连接都通过；如果两方 alpn 都给出, 则只有匹配了才通过


## NativeTLS
同上

NativeTLS 使用 系统自己的 tls栈，这样就没有 rust 特征了。

## http2

服务端用 H2, 客户端用 H2Single 或 H2Mux，一般用 H2Mux 以使用 多路复用

grpc 也是在 http2 配置中设置


### H2

目前 h2 的三种Map 的 Config 格式 是一样的

in

```lua
{
    type = "H2", 
    is_grpc=false,--optional
    http_config={},--optional
}
```
见 [HttpCommonConfig](#httpcommonconfig)

### H2Single

out

```lua
{
    type = "H2Single", 
    is_grpc=false,--optional
    http_config={},--optional
},
```

见 [HttpCommonConfig](#httpcommonconfig)

### H2Mux

out

```lua
{
    type = "H2Mux", 
    is_grpc=false,--optional
    http_config={},--optional
},
```
见 [HttpCommonConfig](#httpcommonconfig)

## WebSocket
in/out


in:

```lua
{
    type = "WebSocket", 
    http_config = {
       --...
    } --optional
}
```
见 [HttpCommonConfig](#httpcommonconfig)


out:

```lua
{
    --... optional
}
```
见 [HttpCommonConfig](#httpcommonconfig)


## Quic

in/out


in:

quic 的 监听端 是直接接管 udp 层的, listen_addr 在这里指定, 而不额外用 Listener


```lua
{
    type = "Quic", 
    key="",
    cert="",
    listen_addr="",
    alpn = { "h2", "h3"},
}
```

out:

```lua
 {
    type = "Quic", 
    server_addr="",
    server_name="www.mytest.com",
    cert="",--optional
    alpn = { "h2", "h3"}, --要明确指定 alpn
    insecure=true,--optional
}
```

须给出 server_name (域名),
 且 若 insecure 为 false, 须为 证书中所写的 CN 或 Subject Alternative Name;
ruci 提供的 test2.crt中的 Subject Alternative Name 为 www.mytest.com 和 localhost,


cert：可给出 服务端的 证书, 这样就算 insecure = false 也通过验证
证书须为 真证书, 或真fullchain 证书, 或自签的根证书

## SPE1: Steganography Protocol Exmaple1

隐写示例协议1

```lua
{ 
    type = "SPE1", 
    qa = { { "q1", "a1" }, { "q2", "a2" } } 
}
```

qa 中要为 2的偶数次幂个 问答对，问答的内容任意填。但是内容越真实，隐写效果越好。


如果不给出qa，则协议会使用自己生成的问答对。

```lua
 { type = "SPE1"}
```


## Lua: lua自定义协议

```lua
{ type = "Lua", file_name = "lua_protocol_e1.lua", handshake_function = "Handshake2" }
```

lua自定义协议 的写法是高级用法，见  [lua自定义协议](user_defined_protocol.md)

# 辅助 Map

## Echo
in/out


## Adder
in/out

```lua
{type = "Adder", value=3}
```

给 输出 的信息 每字节都加 给定的数值。比如 输入abc, value=1, 则输出为 bcd

## Counter

in/out


## HttpFilter

in

```lua
{
    type = "HttpFilter",
    --...
}
```

见 [HttpCommonConfig](#httpcommonconfig)

# 子块

## DnsClient

```lua
dns_client = {
    dns_server_list = { { "127.0.0.1:20800", "udp" } }, -- 8.8.8.8:53
    ip_strategy = "Ipv4Only", --optional
    static_pairs = {
        ['www.baidu.com'] = "103.235.47.188"
    } --optional
}

```
Direct,OptDirect,BindDialer,OptDialer 都可加此块。


ip_strategy 的可能的值：
```rust
pub enum LookupIpStrategy {
    Ipv4Only,
    Ipv6Only,
    Ipv4AndIpv6,
    Ipv6thenIpv4,
    Ipv4thenIpv6,
}
```
不给出时，默认为 Ipv4thenIpv6


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
    authority = "www.myhost.com",
    path = "/ruci_jiandan",

    method = "GET",--optional
    scheme = "https",--optional
    headers= { ["header1"] = "value1", ["header2"] = "value2", },--optional
}
```

# 接下来

现在再读 dev_res/local.lua 就会轻松很多了。

学点难的？
[Infinite](infinite.md)
