
rucimp 的 examples 提供数个示例可执行文件,主要目的是提供演示代码，并提供简单的测试程序

(若要全功能, See [ruci-cmd](crates/ruci-cmd/README.md))


suit, chain 分为以不同的代码运行 suit模式和 chain 模式，

还有 chain_trace 演示 单连接流量监控
chain_infinite 演示 完全动态链, 其与 chain 的运行方式一样, 不再赘述

# 通用

接受 一个 命令行参数, 将其作为配置文件读取, 未提供或者找不到时, 会在工作目录, ruci_config/ , resource/ , ../resource 等 目录下找默认的配置文件.

```sh
# in folder rucimp, run:

# chain mode
RUST_LOG=none,ruci=debug cargo run --features "lua quinn tun" --example chain
RUST_LOG=none,ruci=debug cargo run --features "lua quinn tun" --example chain -- remote.lua

RUST_LOG=none,ruci=debug cargo run --features "lua quinn tun" --example chain_infinite -- local_mux_h2.lua

# linux
RUST_LOG=none,ruci=debug cargo run --features "lua quinn tun sockopt" --example chain

# suit mode
cargo run --example suit -- local.suit.toml
cargo run --example suit -- remote.suit.toml
```

( (h2 的代码实现所依赖的 h2包)、 quic 包、 rustls 等包 都会在debug 下打印大量日志输出, 影响观察ruci本身的日志信息, 
故使用 RUST_LOG=none,ruci=debug 过滤掉非ruci 的 日志)

## route
to use rule_route,

download Country.mmdb from https://cdn.jsdelivr.net/gh/Loyalsoldier/geoip@release/Country.mmdb

then put it to resource folder

## tun

need to enable rucimp's tun feature (which enables ruci's tun feature):

```sh
sudo RUST_LOG=debug cargo run --example chain -F tun -F lua
```

（这里的 -F 与上文的 --features 用处相同，只不过 对于参数 一个适合少量 一个适合大量)

### macos test

使用 [resource/local.lua](../../resource/local.lua) 的对应示例 config_16_tun, inbounds 如

```lua
inbounds = { 
    {chain = { { BindDialer={ dial_addr = "ip://10.0.0.1:24#utun321" } } }, tag = "listen1"} ,
}
```

运行上面命令运行 chain, 然后在 terminal 新标签中 输入下面命令

```sh
sudo ifconfig utun321 10.0.0.1 10.0.0.2 up
ping 10.0.0.2
```

将能在 chain 的命令行中接收到 ping 的数据包

### 全局代理路由

如果您要将您个人电脑的全局网络流量全交由 ruci 代理, 则可以使用 in_auto_route 和 out_auto_route, 或自行配置系统的路由. 

自动路由的配置示例见 [resource/local.lua](../../resource/local.lua)  和 [resource/remote.lua](../../resource/remote.lua) 
(在文件中搜索 auto_route )


# suit 的功能还不全

目前 ruci 项目处于开发阶段, 关注点主要在 chain 模式上面. suit 模式目前只有tcp ,
 没有 udp 和 unix domain socket, 也没有路由 
(suit 模式是仿照verysimple的架构的模式, ruci 目前有此 suit 模式 的意图主要是为了指出，ruci 的新的设计可以
导出 很多其它架构，即ruci 的设计是更一般的设计)
