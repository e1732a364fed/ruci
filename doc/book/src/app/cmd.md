命令行参数 与 程序运行

# 综述

我们 ruci 的可执行文件是 ruci-cmd.

所使用的配置文件的格式 可以是 lua ([lua配置](./lua/lua.md)), 也可以是 toml ([toml配置](./toml/index.md)
).

lua配置更难写一些，但是更灵活一些。 toml 配置更简单一些，但其功能则更少一些。

## 静态链、动态链

ruci 中引入了 “静态链、动态链” 的概念。这两个概念在ruci中至关重要，因此有必要先对其 有一个基本了解。

静态链就是传统的代理的配置方式，而 动态链 则是一种更高级的概念。

toml 配置和基本的 lua配置都是 静态链配置.

而lua配置中还可以通过一些方式开启 动态链的配置。对于新手来说，先把静态链的写法学会就行了。

# 运行方式

本地运行:


    ./ruci-cmd -c local.lua

或

    ./ruci-cmd -c local.toml


服务器运行:


    ./ruci-cmd -c remote.lua

或

    ./ruci-cmd -c remote.toml

macOS 版本要在 系统App：Settings - Privacy & Security 里 allow 一下。

ruci-cmd 会在 下面文件夹中 找 指定的 配置文件

    "./",
    "ruci_config/",
    "resource/",

因此如果不想使用 默认打包的 resource 文件夹，可以将其改名为 resource_default, 
然后 自己创建一个 ruci_config 文件夹，将自己的配置放在 ruci_config 中，这样
就不会产生混淆

另外，配置文件名称是可以自定义的，不一定要叫 local.lua 或 remote.lua, 这只是
一种命名习惯而已。

# 日志

ruci-cmd 运行时产生的日志会自动创建并放在 logs 文件夹中, daily rolling

为了调整日志的等级，在运行参数中 有 两种选择：使用命令行参数 或者使用 环境变量

## 命令行参数法

-l, --log-level <LOG_LEVEL>

可为 ERROR, WARN, INFO , DEBUG, TRACE

## 环境变量法

环境变量法比较高级：

在命令的开头加上

    RUST_LOG=none,ruci=debug 

如

    RUST_LOG=none,ruci=debug ./ruci-cmd -c local.lua

如果是powershell就是加上

    $Env:RUST_LOG="none,ruci=debug";

如

    $Env:RUST_LOG="none,ruci=debug"; .\ruci-cmd.exe -c local.lua

这么写 的效果是：过滤 log, 对其它依赖包的 日志通通不要，只留 ruci 包自己的日志


# utils 工具包

ruci-cmd 提供了一些很方便的命令，可以执行一些辅助功能。

## 生成自签名根证书

    ./ruci-cmd utils gen-cer localhost www.mytest.com

会生成 generated_crt_and_key.crt


## 下载外部依赖资源

    ./ruci-cmd utils mmdb

    ./ruci-cmd utils wintun

## 简易文件服务器

    ./ruci-cmd utils serve-folder
    ./ruci-cmd utils serve-folder 0.0.0.0:12345

serve-folder 命令 会将 ruci-cmd 当前工作目录下的 "static" 文件夹 作为 文件服务器的根路径。

它不会对用户打印出 static 文件夹中的任何文件，而只有当访问 static 中的用户指定的子文件夹时，才会显示其子文件夹的内容。
这样就保护了根路径的内容。

这个文件夹名不可更改，这是为了防止错误地将私密文件暴露。

如果不给出监听地址，会自动监听 "0.0.0.0:18143"。

### 示例链接

查看目录
http://0.0.0.0:18143/folder1


下载
http://0.0.0.0:18143/download/folder1/file1.zip

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

## 生成自签名证书

    ./ruci-cmd utils gen-cer



# 高级用法

lua配置使用 infinite:

    ./ruci-cmd -c local.lua --infinite

运行配置的同时 开启 api-server:

    ./ruci-cmd -c remote.lua -a run 
