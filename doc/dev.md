Clippy:
ruci-cmd 目录下
cargo clippy --all-targets --no-default-features --features "steganography lua file_server api_server api_client utils quinn use-native-tls tun smoltcp"

在 rucimp 目录下
cargo clippy --all-targets --no-default-features --features "tun quinn lua sockopt use-native-tls ruci-rustls21 trace steganography"

或在ruci目录下直接

cargo clippy --all-targets --all-features

注意 rucimp 中不能使用 --all-features 因为 lua 有多个feature, 却只能使用一个



2024.8.28
尝试使用 serde-pickle 但发现生成的 文件在 python 中读取时显示 EOFError: Ran out of input

24.12.25
netstack-lwip 包 无法在 windows 上编译通过, 因此没有加入 ruci-cmd 的feature中 。且经手动测试，发现其性能可能比smoltcp 差一些

在windows 上,tun 包的性能 似乎是因为使用了wintun 的原因，比在 macOS/linux 上要快不少

macOS/linux 上存在内存泄漏，不知如何解决，可能与 tun 包有关。

 从 tun device 有三种方式可以 异步读取，
     1. 在 device 用 AsyncDevice 它自己的 split 方法
     2. 转为 AsyncConn 后 用 tokio 的 split
     3. 转为 Frame 后 分成 sink 和 stream
    
实测3种情况效果相同 但是都卡顿, 且 无论是 netstack-lwip 还是 smoltcp 都有此问题。因此认为是tun 包的问题,与 tcp/ip 栈无关

