
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
    }
}
```

要求每一条链都要有一个 tag

