ruci-cmd 运行时产生的日志会自动创建并放在 logs 文件夹中, daily rolling

# Run and Compile

在shell中进入 crates/ruci-cmd 文件夹.

(用 --infinite 来启用 完全动态链)


```sh
# run with api server
cargo run --features "lua api_server api_client file_server utils use-native-tls steganography lwip" --release -- -a

```

debug:
```sh

# 指定不生成 log 
RUST_LOG=none,ruci=debug cargo run --features "lua utils use-native-tls quinn  steganography lwip smoltcp" -- --log-file ""

# 指定lua配置
RUST_LOG=none,ruci=debug cargo run --features "lua utils use-native-tls quinn  steganography lwip"  -- --log-file "" -c remote.lua

# powershell
$Env:RUST_LOG="none,ruci=debug";cargo run --features "lua utils use-native-tls quinn " -- --log-file ""

# 运行 grpc 的 lua 配置. 注意要加 --infinite
RUST_LOG=none,ruci=debug cargo run --features "lua utils use-native-tls quinn "  -- --log-file "" -c local_mux2_h2.lua --infinite

# 开启所有功能并启用 trace
RUST_LOG=debug cargo run --features "api_server api_client trace lua utils use-native-tls quinn " -- -a --trace
```

make:

```sh
#(for apple silicon)
make BUILD_VERSION=my_version BUILD_TRIPLET=aarch64-apple-darwin
```

详见 Makefile, build_cross.sh 和 .github/workflows/ 中的 脚本

## build lib(.so) for android:
export PATH="$PATH:$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"

arm64:

CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=aarch64-linux-android32-clang cargo build --target aarch64-linux-android --features "api_server utils file_server lwip lua54" --release

x86_64:

CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER=x86_64-linux-android32-clang cargo build --target x86_64-linux-android --features "api_server utils file_server lwip lua54" --release

(feature lua 要用 libc++_shared.so, 所以使用 lua54 feature 编译)

# features

features: lua, lua54, api_server, api_client, utils, trace, use-native-tls, native-tls-vendored, quic, quinn, , smoltcp, lwip
default enables api_server,utils.

api_server, trace 这两个feature都会少许降低 performance. 

trace feature 就算启用了, 
也要在运行ruci-cmd时再加上 --trace 来启用, 因为它一定会影响性能. trace 一般只用于实验/研究/debug

utils feature 启用后，可使用一些子命令下载一些外部依赖文件, 如 `*.mmdb` 和 wintun.dll

## mutually-exclusive-features

use-native-tls, native-tls-vendored

quic, quinn

lua, lua54

### Explained

use-native-tls 在 cross 编译时有问题, 此时只能用 native-tls-vendored

lua 默认情况使用的是 luau, 更快，但在 cross 编译时有问题, 此时只能用 lua54

quic feature 使用的是 s2n-quic, 其不能在windows编译, 且与其它代理程序的quic有一定的互操作性问题, 此时只能用 quinn
(默认情况使用的就是quinn)



# utils

大部分 utils 均有其对应的 api 供远程调用, 格式见 http://127.0.0.1:40681/swagger-ui-ext/

## 下载外部依赖文件

./ruci-cmd utils mmdb

./ruci-cmd utils wintun

./ruci-cmd utils webui
自动下载 ruci-webui 的 发布包，并 解压到 dist 文件夹

## 生成自签名根证书:

./ruci-cmd utils gen-cer localhost www.mytest.com

会生成 generated.crt 和  generated.key


还可以生成 CA证书

./ruci-cmd utils gen-ca My_ORGANIZATION_NAME MyCommonName www.1.com www.2.com


## 配置文件格式转换：

ruci-cmd utils convert-format <INPUT_FILE> <OUTPUT_FORMAT>
如
ruci-cmd utils convert-format local.lua json

lua/json格式在静态链下是可以互相转换的

转后就会生成 local.json. 如果同名文件存在，就会自动用一个新的名称，不会覆盖。

而且也可以  转换为同格式 ，相当于把 注释删掉然后 标准化一下

## 简易文件服务器

    ./ruci-cmd -a

file_server feature 开启后，只要打开 api_server 命令 会将 ruci-cmd 当前工作目录下的 "dist" 文件夹 作为 文件服务器的根路径。

这个文件夹名不可更改，这是为了防止错误地将私密文件暴露。

## 打包

    ./ruci-cmd utils pack folder1
    ./ruci-cmd utils pack-z folder1

pack 和 pack-z 命令 可以对工作目录下的指定文件夹 进行打包。

pack是打包为 tar 文件， pack-z 是在打包为 tar.zip 文件。

它会计算 打包好的 tar 文件的 md5 hash, 并将 该 md5 作为 tar 文件的文件名。

如果是 pack-z, 其依然使用 tar 的 md5 作为 文件名，而不是 zip 的 md5。

## lua 命令行

    ./ruci-cmd utils repl

该命令可以启用一个 lua repl (read, execute, print, loop), 用户可以在里面执行一些lua代码。

## 生成二维码

    ./ruci-cmd utils qr some_string...

# api server

默认api 监听为 127.0.0.1:40681

可用 --api-addr  改变

-a 运行api server

已支持 OpenAPI，运行 api server 后，访问
http://127.0.0.1:40681/swagger-ui
以查看更详细的 api 定义

api:

post /api/engine/start
    启动 rucimp 引擎

/api/engine/stop
    停止 rucimp 引擎

/api/status
    获取服务器状态

/api/traffic/connections/alive/count
    获取活跃连接数

/api/traffic/connections/last/id
    获取最后一个连接 ID

/api/traffic/upload
    获取总上传字节数

/api/traffic/download
    获取总下载字节数

/api/connections/last/ok
    获取最后一个成功的连接 ID

/api/connections
    获取所有连接信息
    (可能返回内容过长，建议使用 connections/count 和 connections/range 代替)

/api/connections/count
    获取连接数量

/api/connections/range/:cid
    获取 CID 大于等于指定值的所有连接信息

/api/connections/:cid
    获取指定 CID 的连接信息

/api/monitoring/status
    获取监控状态 (true/false)

/api/monitoring/enable
    启用监控

/api/monitoring/disable
    禁用监控

/api/traffic/download/:cid
    获取指定 CID 连接的下载流量信息

/api/traffic/upload/:cid
    获取指定 CID 连接的上传流量信息


# 实现细节

utils 和 api_client 模块: 发送http请求用reqwest

api_server 模块: 用了 axum, TinyUFO
