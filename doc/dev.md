Clippy:
ruci-cmd 目录下
cargo clippy --all-targets --no-default-features --features "steganography lua file_server api_server api_client utils quinn use-native-tls lwip smoltcp"

在 rucimp 目录下
cargo clippy --all-targets --no-default-features --features "lwip smoltcp quinn lua sockopt use-native-tls ruci-rustls21 trace steganography"

或在ruci目录下直接

cargo clippy --all-targets --all-features

注意 rucimp 中不能使用 --all-features 因为 lua 有多个feature, 却只能使用一个

还可直接配置 .vscode/settings.json 中的 

```
"rust-analyzer.check.command": "clippy",
```


generate json config files:

in crates/ruci-cmd folder:

```sh
find ../../resource/lua_examples/local -type f -name "*.lua" -exec cargo run --features "lua api_server api_client file_server utils use-native-tls steganography lwip" -- utils convert-format {} json \;
```

2024.8.28
尝试使用 serde-pickle 但发现生成的 文件在 python 中读取时显示 EOFError: Ran out of input

24.12.25
netstack-lwip 包 无法在 windows 上编译通过, 因此没有加入 ruci-cmd 的feature中

macOS/linux 上存在内存泄漏，不知如何解决，可能与 tun 包有关。

 从 tun device 有三种方式可以 异步读取，
     1. 在 device 用 AsyncDevice 它自己的 split 方法
     2. 转为 AsyncConn 后 用 tokio 的 split
     3. 转为 Frame 后 分成 sink 和 stream
    
实测3种情况效果相同 

25.2.24

发现 lua 的 Trojan 的 do_not_use_early_data 在没有给出时， load_static 后 反序列化 
后的 StaticConfig 中 对应的 do_not_use_early_data 变为了 Some(false), 应为 None.

25.3.7
发现 tun 在使用 fd 时，会自动被 drop 掉，导致错误发生。
进而发现，fold_from_start 在 c 不是 Stream::Generator 时，不会
保留c，也没通过 tx 发送，致其被释放

又发现 windows 上开启tun 后，存在大量的组播请求，占用大量资源。这里有问题

新的 netstack-lwip 代码已可以在 windows 上编译通过。
使用了 netstack-smoltcp 包解决 自实现的 smoltcp 的 bug.
但发现其依然有 内存占用 或 内存 泄漏的问题，因此 默认 发布包只 使用 lwip 