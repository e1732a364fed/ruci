# toml 配置
ruci 配置文件的基本命名逻辑是，

local.toml 代表 在客户端 的配置文件

remote.toml 代表在 服务端 的配置文件

基本格式如下：

```toml

[[inbounds]]
chain = []
tag = "in_tag1"

[[outbounds]]
tag = "out_tag1"
chain = []

```

一个简单示例如下：

```toml
[[inbounds]]
chain = [
    { Listener = { listen_addr = "0.0.0.0:10800" } },
    "Counter",
    { Socks5 = {} },
]
tag = "in_tag1"

[[outbounds]]
tag = "out_tag1"
chain = [{ Direct = {} }]

```

## chain

每个 chain 都是一个 列表:

    chain = [ {}, {}, {}]

它是 `MapConfig` 的列表.

如果在 inbound 中，则它是 `InMapConfig` 的列表, 
如果在 outbound 中，则它是 `OutMapConfig` 的列表

写法与lua配置中的写法基本相同，见

[InMapConfig初探](../lua/config_intro.md#InMapConfig初探)
[OutMapConfig初探](../lua/config_intro.md#OutMapConfig初探)

需要注意的是，lua 中的 列表外面的大括号是 `{}`, 而 toml 中的列表的大括号是 `[]`

# 接下来

- [lua配置](../lua/lua.md)
- [路由配置](lua/route_config.md)
