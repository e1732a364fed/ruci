# lua配置

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



中级用法中，Config 中还有 fallback_route, tag_route, rule_route 这几项：

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

# 关于 lua语法

在lua中，大括号 `{}` 被叫做 table, 它即可以当数组用也可以当"字典"用。

行注释以 `--` 开始，块注释如下：
```lua
--[[

]]
```

如写 `x = 1`, 则 x会默认成为 全局变量，这不太好。因此一般都写成
`local x = 1`

字符串就是 `"abc"`, 块级字符串为:
```lua
a = [[
    abcd
    abcd
]]
```

函数是 :

```lua
local function(c)
    return 1
end
```

# 接下来

- [路由配置](route_config.md)

