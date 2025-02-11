# 了解ruci, 并编译运行以试用。

推荐 clone 或者下载 源码zip包之后，在本地阅读 ruci 项目的内容。

首先从 [README.md](../README.md) 读. 之后读 [crates/ruci-cmd/README.md](../crates/ruci-cmd/README.md)

然后读 配置文件 [local.lua](../dev_res/local.lua) 和 [remote.lua](../dev_res/remote.lua)

读完后，只是有了一个大概的印象。还是要编译、运行一下

(或者直接下载release后运行, 运行参数为 [crates/ruci-cmd/README.md](../crates/ruci-cmd/README.md) 文件中 的 cargo run 中后面的 `--` 后面的参数, 不包含 `--`)

先将 local.lua 中 `Config = `的地方设为 `Config = config_0_direct`

之后进入 [crates/ruci-cmd](../crates/ruci-cmd), 运行 [crates/ruci-cmd/README.md](../crates/ruci-cmd/README.md) 中的 标着 “指定不生成 log “ 的那个命令，即可编译+运行（自动运行）

之后把系统代理设为 `socks5://0.0.0.0:10800` 后，即可用本地代理直连了（没什么用，但是可测试程序功能是否正常）

浏览一个网页，发现能正常浏览，之后日志会有 类似下面的内容

```log
ruci-cmd
working dir: “~/ruci/crates/ruci-cmd"
Mode: C
Config: local.lua
LogLevel(flag): None
Empty log-file name specified, no log file would be generated.
Log Level(flag/env): "none,ruci=debug"

…

new accepted stream cid=1 new_cid=1-5
folding…
fold inbound succeed
try select out
folding
direct dial
fold outbound succeed
```

当然，前面会跟着 时间， `DEBUG`, `INFO`, 等字样。

关闭ruci-cmd( ctrl+c) 时，日志会有 如下内容

```log
got interrupt
signal received, starting graceful shutdown...
chain engine: stop called
sending close signal inbound=0
chain engine stopped
terminating listen laddr="0.0.0.0:10800"
Engine reset called
Engine reset successful
chain engine shutted down gracefully
```

这说明一切正常

这就是了解 ruci 的第一步了。

# 读代码

分三步，ruci，rucimp, ruci-cmd

## ruci
先读 net模块，再读 map 模块，然后读 relay 模块

读的时候，先读mod.rs 或每个包的代表文件 的顶部的文档
也可以 用 `cargo doc --no-deps --open` 生成html 文档 后阅读

读完文档后，利用您的代码编辑器纵览文档中指出的关键部分.


## rucimp

rucimp 的核心部分是 modes 模块. 读完 modes 内容后，其它代码都是可选阅读的，如 quic, ws, tproxy 等代码

rucimp 有很多可选feature, 生成文档时要注意, 如

    cargo doc --no-deps --open --features "ruci-rustls21 quinn tun trace lua use-native-tls"

## ruci-cmd (可选)

ruci-cmd 是一套长且乏味的应用程序代码，如果仅需了解基本运行原理，读 rucimp/examples 中的代码就够了

它主要是通过 clap 包 读取命令行命令，然后按需运行相应功能.

功能主要有 start_engine, api_client, api_server, utils 这几个部分.

## 部分重要文档

本项目中散布有很多文档文件，下面列出一些比较重要的

[GOAL_zh.md](GOAL_zh.md)

[CONTRIBUTING_zh.md](CONTRIBUTING_zh.md)

[book](book/src/index.md)

[notes.md](notes.md)

[dev.md](dev.md)

[rucimp/exmaples](../rucimp/examples/readme.md)

[ruci-cmd](../crates/ruci-cmd/README.md)

[ruci-cmd release log](../crates/ruci-cmd/release_log.md)
