
现在我们学习 Config 中几种 route 块的写法:

```lua

routes = {
    tag_route = {},
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


# 接下来

学点难的？
[Infinite](infinite.md)
