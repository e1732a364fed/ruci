
# 用法

本地:


    ./ruci-cmd -c local.lua

服务器:


    ./ruci-cmd -c remote.lua

macOS 版本要在 系统App：Settings - Privacy & Security 里 allow 一下。

ruci-cmd 会在 下面文件夹中 找 指定的 配置文件

    "./",
    "ruci_config/",
    "resource/",

因此如果不想使用 默认打包的 resource 文件夹，可以将其改名为 resource_default, 
然后 自己创建一个 ruci_config 文件夹，将自己的配置放在 ruci_config 中，这样
就不会产生混淆

## log

两种选择，使用命令行参数 或者使用 环境变量

-l, --log-level <LOG_LEVEL>

可为 ERROR, WARN, INFO , DEBUG, TRACE

环境变量法比较高级：

在命令的开头加上

    RUST_LOG=none,ruci=debug 

    powershell
    $Env:RUST_LOG="none,ruci=debug";

这是在过滤 log, 对其它依赖包的 日志通通不要，只留 ruci 包自己的日志



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
