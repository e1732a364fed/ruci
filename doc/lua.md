本文是对lua配置的简单介绍，更完整的介绍可见  [ruci 用户手册](https://e1732a364fed.github.io/ruci/index.html)

Config 是 提供给 rucimp 的项, 静态示例如下

# 静态链

无论lua 中代码怎么写, 对于静态链, 程序只会在lua 代码中找一个全局变量 "Config"

```lua
tls = { type = "TLS" , cert = "test.cert", key = "test.key" }
listen = { type = "Listener" , listen_addr = "0.0.0.0:10800"}
c = {type = "Counter"}
chain1 = {
    listen,
    { type = "Adder",value = 3 },
    c,
    tls,
    c,
    { type = "Socks5",  userpass = "u0 p0", more = {"u1 p1"} },
    {type = "Counter"},

}
len = #chain1
for i=1,5 do 
    chain1[len+1] = tls
    chain1[len+2] = c 
    len = len + 2
    print(len)
end

Config = {
    inbounds = {
        {chain = chain1, tag = "listen1"}
    },
    outbounds = { tag = "d1", chain = { "Blackhole" } },

    tag_route = { { "listen1", "dial1" }, { "listen2", "dial2" }  },
}
```

要求每一条inbound 都要有一个 tag, 每一个 inbound 中的 chain 都要有至少一个 map (映射)


# 动态链

演示动态链的基本用法: 

### 无限(完全)动态链

完全动态链的基本演示完全动态链不使用 固定的列表 来预定义任何Maps, 它只给出一个函数

generator, generator 根据参数内容来动态生成 [Map]; 或者也可以利用 Create_in_map
和 Create_out_map 这两个方法来直接在 lua 中创建map.

Create_* 方法的使用 主要是用于 创建一个map 并缓存起来, 留作之后使用

如果不想重复生成以前生成过的Map, 则可以返回一个已创建过的Map 

下面演示的是 inbound 为 tcp - socks5, outbound 为 direct 的情况

使用 Create_* 方法的示例见 local.lua 和其它 示例 lua 文件

#### 基本演示

```lua

---[[


local inspect = require("inspect")

-- my_cid_record = {}

Infinite = {

    inbounds = {{
        tag = "listen1",

        generator = function(cid, state_index, data)
            if state_index == -1 then
                return 0, {
                    stream_generator = {
                        type = "Listener", listen_addr = "0.0.0.0:10800"
                    },
                    new_thread_fn = function(cid, state_index, data)
                        -- print("lua: cid",inspect(cid))
                        -- table.insert(my_cid_record,cid)
                        -- print("lua: cid cache",inspect(my_cid_record))

                        local new_cid, newi, new_data = coroutine.yield(1, {
                            type = "Socks5"
                        })
                        return -1, {}
                    end
                }
            end
        end
    }},

    outbounds = {{
        tag = "dial1",
        generator = function(cid, state_index, data)
            if state_index == -1 then
                return 0, { type = "Direct"}
            else
                return -1, {}
            end
        end
    }}

}

-- ]]

```