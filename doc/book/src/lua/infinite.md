# 介绍

infinite 模式下，lua配置不使用 Config 变量，而使用 Infinite 变量

基本格式:

```lua
Infinite = {
    inbounds = {
        {
            tag = "listen1",
            generator = function(cid, state_index, data)

            end
        }
    },
    outbounds = {
        {
            tag = "dial1",
            generator = function(cid, state_index, data)

            end
        }
    }
}
```

回顾 [Config 入门](./config_intro.md) ，对比下我们发现，
Infinite 的 基本结构 和 Config 类似，都有 inbounds 和 outbounds 列表。

不过，列表中的元素中的项不再是 tag 和 chain , 而是 tag 和 函数 generator!

generator 将被重复调用，以生成 chain. 区别就在于，通过函数，我们每次生成的
chain 的内容是可以不同的，也就是说，infinite 实现了 动态链 机制！

先看个能用的示例：


下面这个演示 与[Config 入门](./config_intro.md)  中的示例 是等价的：

```lua
local direct = { type = "Direct" }

Infinite = {
    inbounds = { {
        tag = "listen1",
        generator = function(cid, state_index, _data)
            if state_index == -1 then
                return 0, {
                    stream_generator = {
                        type = "Listener", listen_addr = "0.0.0.0:10800"
                    },
                    new_thread_fn = function(cid, state_index, _data)
                        local new_cid, newi, new_data = coroutine.yield(1, {
                            type = "Socks5"
                        })
                        return -1, {}
                    end
                }
            end
        end
    } },
    outbounds = { {
        tag = "dial1",
        generator = function(cid, state_index, _data)
            if state_index == -1 then
                return 0, direct
            else
                return -1, {}
            end
        end
    } }
}
```

首先是要了解，generator 的 返回值有两个，第一个值是 index, 负数表示链结束。
第二个值是 当前链节点 所生成的 Map。

如果是 Listener ，情况则复杂一些，要使用一个 {stream_generator, new_thread_fn }

stream_generator 是一个 正常的 InMapConfig, 而 new_thread_fn 则是一个函数,
它在 stream_generator 每监听到一个新请求时被调用, 因此它内部用到了 lua 的用法
coroutine.yield， 这个就照抄就行。

总之 coroutine.yield 参数中 我们又 返回 一个数和一个 InMapConfig, 
而 coroutine.yield 函数的 返回的时机， 就是我们再次定义下一个 InMapConfig 的时机。
如果链中还有 Map, 我们可以继续调用 coroutine.yield, 如果已经到了 链尾，我们则
直接 `return -1, {}` 就行。

不难，习惯就好。

# 接下来

[lua自定义协议](user_defined_protocol.md)
