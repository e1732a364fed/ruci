Clippy:
在 rucimp 目录下
cargo clippy --all-targets --features "tun quinn lua route geoip sockopt use-native-tls rustls21 trace steganography"

或在ruci目录下直接

cargo clippy --all-targets --all-features

注意 rucimp 中不能使用 --all-features 因为 lua有很多个feature, 却只能使用一个



2024.8.28
尝试使用 serde-pickle 但发现生成的 文件在 python 中读取时显示 EOFError: Ran out of input
