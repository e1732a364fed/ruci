
现在我们学习 Config 中几种 route 块的写法:

```lua

Config = {
    tag_route = {},
    rule_route = {},
    fallback_route = {},
}
```

所有 route 都会用到 chain 的 `tag`
所以我们要求每一条inbound 都要有一个 tag, 每一个 inbound 中的 chain 都要有至少一个 map


# tag_route

```lua
    tag_route = { { "l1", "d1" }, { "l2", "d2" }, { "l3", "d2" } },
```

tag_route 是一个 字符串对 的列表。对中前者为 inbound 的 tag, 后者为 outbound 的 tag。

这是一种固定的 路由模式，只要来自 l1 的 都会被发到 d1.

# fallback_route

首先学一下什么是 fallback. 

在一个 inbound的 chain 中，有序地排列着多个 InMapConfig, 即它们代表着多个 Map,
分别记为 map1, map2. 假设 map1 通过了，但 map2 的协议 逻辑检查失败，即 map2
检查数据，发现和 map2 对应的 协议所定义的 特征不一致，那么此时 整个 chain 就此中断。

如果就这样，一般的情况就是 在 log 中记录一下此次 异常情况, 然后继续 监听 其它请求。

但是，有时，map2 错了，我们依然认为 它是一个有效的 map1, 想要 将它转发到一个新的
outbound 上，此时就用到了 fallback_route. 这整个行为就叫 fallback.

没错。这个就是 trojan 协议的 精髓。


```lua
    fallback_route = { { "listen1", "fallback_dial1" } }

```

fallback_route 是一个 字符串对 的列表。上面示例就是表示 inbound chain "listen1" 里失败的地方将被转发到 
outbound chain "fallback_dial1" 中。listen1 和 fallback_dial1 是它们的 tag.


 
# rule_route

首先给出 与上面的 [tag_route](#tag_route) 等价的 rule_route, 我们来对比学习：

```lua
rule_route = {
    {
        mode = "WhiteList",
        out_tag = "d1",
        in_tags = {"l1"}
    }, {
        mode = "WhiteList",
        out_tag = "d2",
        in_tags = {"l3","l2"}
    }, {
        mode = "WhiteList",
        out_tag = "fallback_d",
        in_tags = {"l1"},
        is_fallback = true
    }
}
```


rule_route 和 tag_route 同时出现时, 程序只会采用 rule_route. 因为 rule_route 的内容涵盖了 tag_route 

rule_route 的 mode 可为 WhiteList 或 BlackList

WhiteList意思是, 给出的规则必须完全匹配, 才算通过.  
BlackList 意思是, 给出的规则有任意一项匹配就算通过.
一般BlackList 用于 路由到 BlackHole, 故名. 


不过，上面的情况我们只给了一个 规则，那就是 in_tags, 只有一个规则时，WhiteList 和 BlackList 是一样的。

is_fallback 若为 true, 则 相当于上面的 [fallback_route](#fallback_route) 模式。

因此可以发现, rule_route 可以替换掉 tag_route 和 fallback_route, 属于更高级、更通用的 配置。


下面是一个复杂的情况，有多个规则（rule_route 中 除了 out_tag，mode 以外，其它的项 都是规则）：

```lua
Config = {
    inbounds = {
        {chain = chain1, tag = "listen1"}
    },
    outbounds = { tag = "d1", chain = { "Blackhole" } },

    tag_route = { { "listen1", "dial1" }, { "listen2", "dial2" }  },

    rule_route = { 
        { 
            out_tag = "dial1", 
            mode = "WhiteList",

            in_tags = { "listen1" } ,
            is_fallback = false,
             userset = {
                { "plaintext:u0 p0", "trojan:mypassword" },
                { "plaintext:u1 p1", "trojan:password1" },
            },
            ta_ip_countries = { "CN", "US" },
            ta_networks = { "tcp", "udp" },
            ta_ipv4 = { "192.168.1.0/24" },
            ta_domain_matcher = {
                domain_regex = {  "[a-z]+@[a-z]+",
                "[a-z]+" },
                domain_set = { "www.baidu.com" },
            }
        } 
    }
}
```

这里 ta_ 开头的 四项 都是和 目标地址 有关的，它们是 通过请求网址 的 域名、ip地区 分流 的常见做法。

userset 用于判断 在 InChain 中 那些需要 密码 的 Map 中 使用了 哪些 密码 的用户 属于本分流


# 接下来

学点难的？
[Infinite](./infinite.md)
