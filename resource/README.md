此文件夹包含若干ruci运行需要的文件

local*.lua, remote.lua 为 chain 模式的示例配置文件

local.suit.toml, remote.suit.toml 为 suit 模式的示例配置文件

test.crt, test.key 用于测试用于tls的自签名证书

test.crt 为 pem 格式的 x509 证书, test.key 为 pem 格式的 EC key

inspect.lua 是一个lua模块, 来自
//https://raw.githubusercontent.com/kikito/inspect.lua/master/inspect.lua

可以帮助在 lua中打印一个值的内容

在ruci-cmd中, 下载的 Country.mmdb 和 wintun.dll 也会放在这里

