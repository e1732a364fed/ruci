
config 是 提供给 rucimp 的项，静态示例如下

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

要求每一条inbound 都要有一个 tag, 每一个 inbound 中的 chain 都要有至少一个 mapper (映射函数)





