# 订阅

ruci 中使用一种非常简单的方法来实现订阅，分为三步：打包、服务文件（生成订阅url）、下载。



## 示例流程
```sh
./ruci-cmd utils pack-z resource
mkdir static
mv 83c649c74b8a4c6ebc07a9a99ee350a0.tar.zip static/
./ruci-cmd utils serve-folder
./ruci-cmd -c http://0.0.0.0:18143/download/83c649c74b8a4c6ebc07a9a99ee350a0.tar.zip --in-memory
```

## 打包

    ./ruci-cmd utils pack-z resource

它会把 ruci-cmd 目录下的 resource 文件夹中的全部内容打包为一个 {md5}.tar.zip 文件。
这个 zip文件里面只有一个文件，即 {md5}.tar， 而 {md5}.tar 里面则是 resource 文件夹中的所有文件内容（不包含resource 这个层级）

其中 {md5} 是 {md5}.tar 这个文件的 md5 哈希值。

## 服务文件（生成订阅url）

之后把 {md5}.tar.zip 文件移动到 ruci-cmd 所在目录的 static 文件夹下。若没有则创建一个。

```sh
mkdir static
mv 83c649c74b8a4c6ebc07a9a99ee350a0.tar.zip static/
```

然后运行 ruci-cmd 的文件服务器

```sh
./ruci-cmd utils serve-folder
```

这样，订阅链接就会自动为 
http://0.0.0.0:18143/download/83c649c74b8a4c6ebc07a9a99ee350a0.tar.zip

注意，0.0.0.0 是本机地址，如果您要在公网，可以将ip换为 您的公网ip。

ruci 不提供https、用户鉴权的机制，您可以通过一些反向代理的方式来提供安全性。


## 在客户端使用订阅链接

正常使用配置文件时，是用的 `ruci-cmd -c local.lua`, 此时，只要把 -c 的参数改为对应的下载链接即可：

```sh
./ruci-cmd -c http://0.0.0.0:18143/download/83c649c74b8a4c6ebc07a9a99ee350a0.tar.zip
```

该命令会下载该压缩包、保存到当前目录并使用。它会自动阅读包中的 local.lua 或 local.toml 文件。

如果不想把下载的包保存，则可以加 ` --in-memory` 选项。

如果已经保存了下载好的包，下一次使用时可以直接解压缩里面的内容使用，也可以直接用

```sh
./ruci-cmd -c 83c649c74b8a4c6ebc07a9a99ee350a0.tar.zip
```

或者

```sh
./ruci-cmd -c 83c649c74b8a4c6ebc07a9a99ee350a0.tar
```

注意，如果不解压缩，则不要修改 tar 或 tar.zip 的名字。因为名字要作为 md5 由 ruci-cmd 检查其包内容是否一致。
如果 包的实际内容的 md5 与 名字不一致，则 ruci-cmd 会拒绝运行。

# 接下来

- [toml配置](../toml/index.md)
- [lua配置](../lua/lua.md)
- [路由配置](lua/route_config.md)