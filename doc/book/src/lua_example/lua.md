ruci 配置文件的基本命名逻辑是，

local.lua 代表 在客户端 的配置文件

remote.lua 代表在 服务端 的配置文件

入门用法如下, 使用 Config 变量:

```lua

Config = {
    inbounds = {},
    outbounds = {},

}
```

中级用法中，还有 fallback_route, tag_route, rule_route
```lua

Config = {
    inbounds = {},
    outbounds = {},
    fallback_route = {},
    tag_route = {},
    rule_route = {},
}
```

高级用法中，还有[Infinite 配置](./infinite.md) 。

先学 简单的 [Config 入门](./config_intro.md) 吧。

