
infinite 模式下，lua配置不使用 Config 变量，而使用 Infinite 变量

基本格式:

```lua
Infinite = {
    inbounds = {
        tag = "listen1",
        generator = function(cid, state_index, data)

        end
    },
    outbounds = {
        tag = "dial1",
        generator = function(cid, state_index, data)

        end
    }
}
```

