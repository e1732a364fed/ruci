
rucimp 提供数个示例可执行文件, 若要全功能, 用 rucimple

suit2, chain 分为以不同的代码运行 suit模式和 chain 模式



# 通用

接受 一个 命令行参数，将其作为配置文件读取，未提供或者找不到时，会在工作目录, /resource, ../resource 目录下找默认的配置文件.

```sh
# in folder rucimp, run:

# chain mode
cargo run --example chain -- local.lua
cargo run --example chain -- remove.lua

# suit mode
cargo run --example suit2 -- local.suit.toml
cargo run --example suit2 -- remote.suit.toml
```

# chain 程序

## -s

可接受一个 -s 参数，表示 永远sleep

在 用 Stdio 作为 listen 的chain 的起点时，要用-s才行，即:

listen 的 chain 为类似
```lua
stdin_adder_chain = { { Stdio="fake.com:80" } , { Adder = 1 } }
```
时，要用如下命令

```sh
    cargo run --example chain --  -s
```
