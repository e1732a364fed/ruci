

features: api_server, api_client, utils
default enables none.

utils feature 可用于下载一些外部依赖文件, 如 `*.mmdb` 和 wintun.dll


run with api server :3000

```
RUST_LOG=debug cargo run -F api_server -F api_client -F utils -F trace -- -a run
```

# api server

api:

/c

    get all connection's info

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