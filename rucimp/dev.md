Clippy:
在 rucimp 目录下
cargo clippy --all-targets --features "tun quinn lua route geoip sockopt use-native-tls rustls21 trace steganography"

或在ruci目录下直接

cargo clippy --all-targets --all-features

注意 rucimp 中不能使用 --all-features 因为 lua 有多个feature, 却只能使用一个



2024.8.28
尝试使用 serde-pickle 但发现生成的 文件在 python 中读取时显示 EOFError: Ran out of input

24.12.25
lwip 无法在 windows 上编译通过, 因此没有加入 ruci-cmd 的feature中 。且经手动测试，发现其性能可能比smoltcp 差一些

在windows 上,tun 包的性能 似乎是因为使用了wintun 的原因，比在 macOS/linux 上要快不少
