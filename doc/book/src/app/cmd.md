
# 用法

本地:


    ./ruci-cmd -c local.lua

服务器:


    ./ruci-cmd -c remote.lua

## 高级用法

使用 infinite:

    ./ruci-cmd -c local.lua --infinite

运行配置的同时 开启 api-server:

    ./ruci-cmd -c remote.lua -a run 

# 说明

ruci-cmd 运行时产生的日志会自动创建并放在 logs 文件夹中, daily rolling



# utils

生成自签名根证书:

    ./ruci-cmd utils gen-cer localhost www.mytest.com

会生成 generated_crt_and_key.crt


下载外部依赖资源:

    ./ruci-cmd utils mmdb

    ./ruci-cmd utils wintun
