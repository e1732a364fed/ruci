
config 是 提供给 rucimp 的项，静态示例如下

# 静态链

无论lua 中代码怎么写，对于静态链, 程序只会在lua 代码中找一个全局变量 "config"

```lua
tls = { TLS = {  cert = "test.cert", key = "test.key" } }
listen = { Listener   "0.0.0.0:1080" }
c = "Counter"
chain1 = {
    listen,
    { Adder = 3 },
    c,
    tls,
    c,
    { Socks5 = {  userpass = "u0 p0", more = {"u1 p1"} } },
    "Counter",

}
len = table.getn(chain1)
for i=1,5 do 
    chain1[len+1] = tls
    chain1[len+2] = c 
    len = len + 2
    print(len)
end

config = {
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
            ta_ip_countries = { "CN", "US" }, --ta means target_addr
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

要求每一条inbound 都要有一个 tag, 每一个 inbound 中的 chain 都要有至少一个 mapper (映射函数)

rule_route 和 tag_route 同时出现时，程序只会采用 rule_route. 因为 rule_route 的内容涵盖了 tag_route 

rule_route 的 mode 可为 WhiteList 或 BlackList

WhiteList意思是，给出的规则必须完全匹配，才算通过。 
BlackList 意思是，给出的规则有任意一项匹配就算通过.
一般BlackList 用于 路由到 BlackHole, 故名。

# 动态链

演示动态链的基本用法：

## 有限(局部)动态链

有限动态链中，程序一样会读取 config 变量，和 静态链一样。但它还要读一个 dyn_selectors 全局函数

dyn_selectors, 对每一个tag 的inbound/outbound 都要有返回一个selector

selector 接受 this_index 和 data 作为输入, 返回一个新的index, index 是 在 该chain (Mapper数组) 中的索引

    selector 在 "有限状态机" 里对应的是 "状态转移函数"


示例：

```lua

---[[

-- 演示 有限动态链的 选择器用法


function dyn_selectors(tag)
    if tag == "listen1" then 
        return dyn_inbound_next_selector
    end
     if tag == "dial1" then 
        return dyn_outbound_next_selector
    end
end

-- 下面两个selector 示例都是 最简单的示例, 使得动态链的行为和静态链相同

dyn_inbound_next_selector = function (this_index, data)
    -- print("data:",data)
   
    return this_index + 1
end

dyn_outbound_next_selector = function (this_index, data)
    return this_index + 1
end


--]]
```

### 无限(完全)动态链

完全动态链的基本演示完全动态链不使用 固定的列表 来预定义任何Mappers, 它只给出一个函数

generator, generator 根据参数内容来动态生成 [Mapper], 如果不想

重复生成以前生成过的Mapper, 则可以返回一个已创建过的Mapper 

演示的功能是 inbound 为 tcp - socks5, outbound 为 direct

#### 基本演示

```lua

---[[


local inspect = require("inspect")

-- my_cid_record = {}

infinite = {

    inbounds = {{
        tag = "listen1",

        generator = function(cid, state_index, data)
            if state_index == -1 then
                return 0, {
                    stream_generator = {
                        Listener = "0.0.0.0:10800"
                    },
                    new_thread_fn = function(cid, state_index, data)
                        -- print("lua: cid",inspect(cid))
                        -- table.insert(my_cid_record,cid)
                        -- print("lua: cid cache",inspect(my_cid_record))

                        local new_cid, newi, new_data = coroutine.yield(1, {
                            Socks5 = {}
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
                return 0, "Direct"
            else
                return -1, {}
            end
        end
    }}

}

-- ]]

```