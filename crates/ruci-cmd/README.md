
ruci-cmd 运行时产生的日志会自动创建并放在 logs 文件夹中, daily rolling

# Run and Compile

在shell中进入 crates/ruci-cmd 文件夹.

(用 --infinite 来启用 完全动态链)


```sh
# run with api server
cargo run --features "lua api_server api_client utils use-native-tls" --release -- -a

```

debug:
```sh

# 指定不生成 log 
RUST_LOG=none,ruci=debug cargo run --features "lua utils use-native-tls quinn tun steganography smoltcp" -- --log-file ""

# 指定lua配置
RUST_LOG=none,ruci=debug cargo run --features "lua utils use-native-tls quinn tun steganography smoltcp"  -- --log-file "" -c remote.lua

# powershell
$Env:RUST_LOG="none,ruci=debug";cargo run --features "lua utils use-native-tls quinn tun" -- --log-file ""

# 运行 grpc 的 lua 配置. 注意要加 --infinite
RUST_LOG=none,ruci=debug cargo run --features "lua utils use-native-tls quinn tun"  -- --log-file "" -c local_mux2_h2.lua --infinite

# 开启所有功能并启用 trace
RUST_LOG=debug cargo run --features "api_server api_client trace lua utils use-native-tls quinn tun" -- -a --trace
```

make:

```sh
#(for apple silicon)
make BUILD_VERSION=my_version BUILD_TRIPLET=aarch64-apple-darwin
```

详见 Makefile, build_cross.sh 和 .github/workflows/ 中的 脚本

# features

features: lua, lua54, api_server, api_client, utils, trace, use-native-tls, native-tls-vendored, quic, quinn, tun, smoltcp
default enables none.

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

## 下载外部依赖文件

./ruci-cmd utils mmdb

./ruci-cmd utils wintun

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

    ./ruci-cmd utils serve-folder
    ./ruci-cmd utils serve-folder 0.0.0.0:12345

serve-folder 命令 会将 ruci-cmd 当前工作目录下的 "static" 文件夹 作为 文件服务器的根路径。

它不会对用户打印出 static 文件夹中的任何文件，而只有当访问 static 中的用户指定的子文件夹时，才会显示其子文件夹的内容。
这样就保护了根路径的内容。

这个文件夹名不可更改，这是为了防止错误地将私密文件暴露。

如果不给出监听地址，会自动监听 "0.0.0.0:18143"。

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

默认api 监听为 127.0.0.1:40681 , file_server 监听默认为 0.0.0.0:18143

可用 --api-addr 和 --file-server-addr 改变

-a 运行api server

api:

post /start_engine
    start_engine

/stop_engine

    stop rucimp engine

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

utils 和 api_client 模块: 发送http请求用reqwest

api_server 模块: 用了 axum, TinyUFO
