
rucimp 提供数个示例可执行文件, 若要全功能, 用 rucimple

suit, chain 分为以不同的代码运行 suit模式和 chain 模式



# 通用

接受 一个 命令行参数，将其作为配置文件读取，未提供或者找不到时，会在工作目录, ruci_config/ , resource/ , ../resource 等 目录下找默认的配置文件.

```sh
# in folder rucimp, run:

# chain mode
cargo run --example chain -- local.lua
cargo run --example chain -- remote.lua

# suit mode
cargo run --example suit -- local.suit.toml
cargo run --example suit -- remote.suit.toml
```

to use rule_route,

download Country.mmdb from https://cdn.jsdelivr.net/gh/Loyalsoldier/geoip@release/Country.mmdb

then put it to resource folder

