# toml 配置
ruci 配置文件的基本命名逻辑是，

local.toml 代表 在客户端 的配置文件

remote.toml 代表在 服务端 的配置文件

基本格式如下：

```toml

[[inbounds]]
chain = []
tag = "in_tag1"

[[outbounds]]
tag = "out_tag1"
chain = []

```
