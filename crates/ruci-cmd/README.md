
发送http请求用reqwest, 接收用 axum

features: api_server, api_client, utils
default enables all.

utils feature 可用于下载一些外部依赖文件, 如 `*.mmdb` 和 wintun.dll


run with api server :3000

```
RUST_LOG=debug cargo run -F api_server -F api_client -F utils -F trace -- -a run
```