
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

# 动态链

演示动态链的基本用法：

## 有限动态链

有限动态链中，程序一样会读取 config 变量，和 静态链一样，但还要读一个 dyn_selectors 函数

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

dyn_inbound_next_selector = function (this_index, ovov)
   -- print("ovov:is_some()",ovov:is_some())

    if ovov:is_some()  then
       -- print("ovov:len()",ovov:len())

        if ovov:len() > 0 then
            ov = ovov:get(0)
           -- print("ov:has_value()",ov:has_value())

            if ov:has_value() then
                the_type = ov:get_type()
              --  print(the_type)

                if the_type == "data" then
                    d = ov:get_data()
              --      print(d:get_u64())
                end
            end
        end
    end
   
    return this_index + 1
end

dyn_outbound_next_selector = function (this_index, ovov)
    return this_index + 1
end


--]]
```
