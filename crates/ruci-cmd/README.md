
产生的日志会的 logs 文件夹中, daily rolling

# Run and Compile

用 --infinite 来启用 完全动态链

run with api server:

```sh
cargo run --features "lua api_server api_client utils use-native-tls" --release -- -a run

```

debug:
```sh
RUST_LOG=none,ruci=debug cargo run --features "lua utils use-native-tls quinn tun" -- --log-file ""

RUST_LOG=none,ruci=debug cargo run --features "lua utils use-native-tls quinn tun"  -- --log-file "" -c remote.lua

#powershell
$Env:RUST_LOG="none,ruci=debug";cargo run --features "lua utils use-native-tls quinn tun" -- --log-file ""


RUST_LOG=none,ruci=debug cargo run --features "lua utils use-native-tls quinn tun"  -- --log-file "" -c local_mux2_h2.lua --infinite

RUST_LOG=debug cargo run --features "api_server api_client trace lua utils use-native-tls quinn tun" -- -a run --trace

# with tproxy(linux):

RUST_LOG=none,ruci=debug cargo run --features "lua utils use-native-tls quinn tun tproxy" -- --log-file ""

RUST_LOG=none,ruci=debug cargo run --features "lua utils use-native-tls quinn tun tproxy" -- --log-file "" -c remote.lua

```

make:

```sh
#(for apple silicon)
make BUILD_VERSION=my_version BUILD_TRIPLET=aarch64-apple-darwin
```

详见 Makefile, build_cross.sh 和 .github/workflows/ 中的 脚本

# features

features: lua, lua54, api_server, api_client, utils, trace, use-native-tls, native-tls-vendored, quic, quinn, tun
default enables none.

api_server, trace 这两个feature都会少许降低 performance. 

trace feature 就算启用了, 
也要在运行ruci-cmd时再加上 --trace 来启用, 因为它一定会影响性能. trace 一般只用于实验/研究/debug

utils feature 可用于下载一些外部依赖文件, 如 `*.mmdb` 和 wintun.dll

## mutually-exclusive-features

use-native-tls, native-tls-vendored

quic, quinn

lua, lua54

### Explained

use-native-tls 在 cross 编译时有问题, 此时只能用 native-tls-vendored

lua 使用的是 luau, 更快，但在 cross 编译时有问题, 此时只能用 lua54

quic feature 使用的是 s2n-quic, 其不能在windows编译, 且与其它代理程序的quic有一定的互操作性问题, 此时只能用 quinn



# utils

生成自签名根证书:

./ruci-cmd utils gen-cer localhost www.mytest.com

会生成 generated.crt 和 generated.key

./ruci-cmd utils mmdb

./ruci-cmd utils wintun


# api server

默认api 监听为 127.0.0.1:40681 , file_server 监听默认为 0.0.0.0:18143

可用 --api-addr 和 --file-server-addr 改变

-a run 运行

-a file-server 来运行 file server. 

可以 -a file-server -a run 来同时运行 file server 和 api server , 但 file-server 必须在 run 前给出

api:

/stop_core

    stop rucimp core

/gt/acc

    all connection count

/gt/lci

    last conn id

/gt/u

    total upload bytes

/gt/d

    total download bytes

/loci

    get last ok cid

/all_c

    get all connection's info
    (might be too long, try use cc and cr instead)

/cc

    connections number

/cr/3

    get infos for all connections whose cid is after cid: 3

/c/1

    get info for connection with cid: 1

/m
    
    get monitor state (true/false)

/m_on
    
    enable monitor

/m_off
    
    disable monitor

/d/1
    
    get download flux for connection with cid: 1

/u/1
    
    get upload flux for connection with cid: 1


# 实现细节

utils, api_client: 发送http请求用reqwest

api_server: 用了 axum, TinyUFO
