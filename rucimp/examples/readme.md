
rucimp 提供数个示例可执行文件, 若要全功能, 用 ruci-cmd

suit, chain 分为以不同的代码运行 suit模式和 chain 模式

还有 chain_trace 演示 单连接流量监控
chain_infinite 演示 完全动态链，其与 chain 的运行方式一样，不再赘述

# 通用

接受 一个 命令行参数，将其作为配置文件读取，未提供或者找不到时，会在工作目录, ruci_config/ , resource/ , ../resource 等 目录下找默认的配置文件.

```sh
# in folder rucimp, run:

# chain mode
RUST_LOG=none,ruci=debug cargo run -F lua --example chain
RUST_LOG=none,ruci=debug cargo run -F lua --example chain -- remote.lua

RUST_LOG=none,ruci=debug cargo run -F lua --example chain_infinite -- local_mux_h2.lua


# suit mode
cargo run --example suit -- local.suit.toml
cargo run --example suit -- remote.suit.toml
```

(h2 的代码实现所使用的 h2包 会在debug 下打印大量日志输出, 影响观察，故使用 RUST_LOG=none,ruci=debug 过滤掉非
ruci 的 日志)

## route
to use rule_route,

download Country.mmdb from https://cdn.jsdelivr.net/gh/Loyalsoldier/geoip@release/Country.mmdb

then put it to resource folder

## tun

need to enable rucimp's tun feature (which enables ruci's tun feature):

```sh
sudo RUST_LOG=debug cargo run --example chain -F tun -F lua
```

### macos test

使用 local.lua 的对应示例 config, inbounds 如

```lua
inbounds = { 
    {chain = { { Dialer="ip://10.0.0.1:24#utun321" } }, tag = "listen1"} ,
}
```

运行上面命令运行 chain, 然后在terminal 输入下面命令

```sh
sudo ifconfig utun321 10.0.0.1 10.0.0.2 up
ping 10.0.0.2
```

将能在 chain 的命令行中接收到 ping 的数据包

### 全局代理路由

如果您要将您个人电脑的全局网络流量全交由 ruci 代理, 则您需要自行配置系统的路由。

在未来的 ruci-cmd 项目中, 会添加 自动全局路由功能


# suit 的功能还不全

目前 ruci 项目处于开发阶段, 关注点主要在 chain 模式上面. suit 模式目前只有tcp ,
 没有 udp 和 unix domain socket, 也没有路由
