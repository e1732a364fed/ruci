

features: api_server, api_client, utils, trace
default enables none.

api_server, trace 这两个feature都会少许降低 performace. 

trace feature 就算启用了，
也要在运行ruci-cmd时再加上 --trace 来启用, 因为它一定会影响性能. trace 一般只用于实验/研究/debug

utils feature 可用于下载一些外部依赖文件, 如 `*.mmdb` 和 wintun.dll

用 --dyn-config 来启用 完全动态链

run with api server:

```
cargo run -F api_server -F api_client -F utils --release -- -a run

```

debug:
```
RUST_LOG=debug cargo run

RUST_LOG=debug cargo run -F api_server -F api_client -F utils -F trace -- -a run --trace

```

make:

```sh
#(for apple silicon)
make BUILD_VERSION=myversion BUILD_TRIPLET=aarch64-apple-darwin
```

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

/allc

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

发送http请求用reqwest, 接收用 axum
用了 TinyUFO