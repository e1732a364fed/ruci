# Config入门

一般情况下，每个 配置文件里都要写一个 Config 块, 程序 读取解析这个 Config 后就运行。

让我们创建一个 `local.lua` 文件，内容如下:

```lua

Config = {
    inbounds = {},
    outbounds = {},

}
```

inbounds 是指 入站, 即 在这里设置 本地监听 的端口

outbounds 是指 出站, 即 在这里设置 远程服务器 的地址

因为可能有多个 入站和出站，所以 inbounds 和 outbounds 都是 列表，
也就是说，下面的示例分别有两个入站 和 三个出站

```lua

Config = {
    inbounds = { {}, {}}
    outbounds = { {}, {}, {}}
}

```

直接看 [最简配置](#最简配置), 还是一步步学起？你说的算~


# inbounds/outbounds

inbounds/outbounds 是 [inbound/outbound](#inboundoutbound) 的列表:

    inbounds = { inbound1, inbound2, ... }

## inbound/outbound

每个 inbound/outbound 都由 一个 [chain](#chain) 和一个 tag 组成:

```lua
{
    chain = {},
    tag = "listen1"
}
```

## chain

每个 chain 都是一个 列表:

    chain = { {}, {}, {}, ..}

它是 `MapConfig` 的列表.

如果在 inbound 中，则它是 `InMapConfig` 的列表, 
如果在 outbound 中，则它是 `OutMapConfig` 的列表

每一种 `MapConfig` 都是一个 由 大括号 括起来的 table

我们先学简单的几个 Config

## InMapConfig初探

先学两种 InMapConfig，Listener 和 Sock5Http

### Listener

下面配置 监听 本地 tcp 端口 10800:

    {
        type = "Listener", listen_addr = "0.0.0.0:10800"
    }

### Sock5Http

Sock5Http 可读取 socks5 协议 和 http代理 协议

    {
        type = "Socks5Http"
    }

### 合体

知道了 Listener 和 Sock5Http 这两个 InMapConfig 后，我们把它
合起来放到一个 chain 中：

```lua
chain = {
    {
        type = "Listener", listen_addr = "0.0.0.0:10800"
    },
    {
        type = "Socks5Http"
    }
}
```
再把它包起来变成一个 inbound ：

```lua
{
    tag = "listen1",
    chain = {
        {
            type = "Listener", listen_addr = "0.0.0.0:10800"
        },
        {
            type = "Socks5Http"
        }
    }
}
```


再把它放入 Config 的 inbounds 中, 作为 目前 inbounds 唯一的 inbound：

```lua
Config = {
    inbounds = {
        {
            tag = "listen1",
            chain = {
                {
                    type = "Listener", listen_addr = "0.0.0.0:10800"
                },
                {
                    type = "Socks5Http"
                }
            }
        }
    },

}
```


这样 我们第一个 inbounds 配置就做好了！

## OutMapConfig初探

先学 最简单的 `OutMapConfig` Direct:

### Direct

Direct 是最简单的 OutMap! 它就是直连：

    { type = "Direct" }

### 合体

它 Direct 块 放入 chain 中：

```lua
chain = {
    { type = "Direct" },
}
```

再把它放入 Config 的 outbounds 中：

```lua
Config = {
    outbounds = {
        {
            tag = "direct",
            chain =  {
                { type = "Direct" },
            }
        }
    },

}
```

再和 上面 inbounds 结合起来，第一个能用的 lua配置就做好了：

```lua
Config = {
    inbounds = {
        {
            tag = "listen1",
            chain = {
                {
                    type = "Listener", listen_addr = "0.0.0.0:10800"
                },
                {
                    type = "Socks5Http"
                }
            }
        }
    },
    outbounds = {
        {
            tag = "direct",
            chain =  {
                { type = "Direct" },
            }
        }
    },
}
```

# 使用 lua 变量

上面写法是不是太长了？不要急！我们使用lua不是为了让配置变复杂，而是让它变简单，方法就是用变量替换！

我们把每个有意义的子块都给个 `变量名`：

```lua
local direct = { type = "Direct" }
local listener = {  type = "Listener", listen_addr = "0.0.0.0:10800"  }
local sock5http = { type = "Socks5Http" }

```

（local 的用法不在此解释，照抄就行）

如此，整个 配置就变成了

```lua

local direct = { type = "Direct" }
local listener = {  type = "Listener", listen_addr = "0.0.0.0:10800"  }
local sock5http = { type = "Socks5Http" }

Config = {
    inbounds = {
        {
            tag = "listen1",
            chain = {   listener,  sock5http  }
        }
    },
    outbounds = {
        {
            tag = "direct",
            chain =  {  direct,  }
        }
    },
}
```

## 最简配置

再替换一次：

```lua
local direct = { type = "Direct" }
local listener = {  type = "Listener", listen_addr = "0.0.0.0:10800"  }
local sock5http = { type = "Socks5Http" }

local listen_inbound = { tag = "listen1", chain = { listener, sock5http } }
local direct_outbound = { tag = "direct", chain =  { direct, } }

Config = {
    inbounds = { listen_inbound },
    outbounds = { direct_outbound },
}
```

怎么样，是不是一下子就变得 清晰起来了？爽！先来个 high-five 吧！

配置您电脑的 系统代理 为 socks5或 http, 指向 `127.0.0.1:10800`， 

然后运行

    ./ruci_cmd -c local.lua

windows:

    ruci_cmd.exe -c local.lua

测试成功！


下一步，学习 [各个 `MapConfig` 的写法 ](map_config.md)
或者直接开始学 [各个 route 的写法](route_config.md)？
