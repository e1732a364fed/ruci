# ruci 入门

ruci 的可执行文件叫做 ruci-cmd, 客户端和服务端都是使用这同一个程序。

## 安装ruci

从这里下载 ruci-cmd 的最新发布版：[Release](https://github.com/e1732a364fed/ruci/releases)


发布版 以 .tar.xz 为后缀，是一个压缩包，包含 ruci-cmd 以及相关的 resource 文件夹。

压缩包对于不同的平台有不同的后缀。

### windows

建议下载后缀为 `x86_64-pc-windows-msvc.tar.xz` 的版本

下载后，可以用 7zip 来解压，先解压出一个 tar, 再解压一遍得到程序。(或双击用7zip 打开后，进入tar之后再将其内容拖拽出来)

第一次运行时，windows 可能弹出提示，询问是否允许连接到网络，同意即可。


### macOS

apple silicon（m1,m2,m3,m4） 下载
`aarch64-apple-darwin.tar.xz` 

老机型下载
`x86_64-apple-darwin.tar.xz `

解压：

    tar xf archive.tar.xz

第一次运行时，macOS 会提示您该程序不受信任，您可以到 设置-隐私与安全 中，许可本程序的使用。

注：编译运行则不会出现此提示。

### x64 linux
建议 x64的 linux 用户下载 后缀为 `x86_64-unknown-linux-gnu.tar.xz` 的版本

如果运行闪退，则可以下载 后缀为 `x86_64-unknown-linux-musl.tar.xz` 的版本

解压：

    tar xf archive.tar.xz
    chmod +x ruci-cmd

### 安卓

termux 用户可以下载 后缀为 `aarch64-linux-android.tar.xz` 的版本

可以先在电脑上解压好，再传到手机中

之后在 termux 中

    chmod +x ruci-cmd


## 开始使用

在ruci-cmd 所在的文件夹中：

windows下，打开 cmd 或 powershell, 运行：

    .\ruci-cmd.exe

其它平台，进入终端，输入

    ./ruci-cmd

为了保持本文的简洁，下面命令行示例统一使用 linux 的格式。


运行ruci-cmd后它会在相同目录下的 resource 文件夹 或 ruci_config 文件夹寻找 `local.lua` 文件，如果
找到了，就会运行，否则就会退出。

运行时，会同时在 ruci-cmd 的当前目录下生成一个 logs 文件夹，用于存放生成的日志文件。

如果要指定配置文件运行，可以加 -c 参数：

    ./ruci-cmd -c remote.lua
    ./ruci-cmd -c local.json

如果要了解编译等方面的细节，可参考 [这里](https://github.com/e1732a364fed/ruci/blob/tokio/crates/ruci-cmd/README.md)

为了不让 resource 文件夹中的示例文件影响您的自定义配置，您可以把 resource 文件夹重命名为其它名称，
然后建立一个 ruci_config 文件夹，将您的配置文件放在 ruci_config 文件夹中。


resource 文件夹中的内容有助于参考使用，建议保留。

调节日志等级为 debug:

    ./ruci-cmd -l debug

# ruci-gui

另一种使用 ruci 的方式是使用 gui, 来自 [ruci-webui](https://github.com/e1732a364fed/ruci-webui/) 项目，
它使用 tauri 编译了 桌面和 安卓平台的 gui，内置了 ruci内核

到 [https://github.com/e1732a364fed/ruci-webui/releases/](https://github.com/e1732a364fed/ruci-webui/releases/) 下载最新的编译版本。

启动该gui后，可在 Control Panel 中 点击 “检查服务器状态”，它会显示 `服务器状态: {"status":"running"}`
这表示 内核已经正在运行。然后 点击 “选择配置文件”，再点击 “启动引擎”，就可以运行 您的 ruci 配置了。

ruci-gui 的 Node Editor 还提供了一种很方便的 “节点编辑器”，可以 以可视化的方式编辑您的配置文件。
而 Control Panel 中又提供了一些方便的小工具。


# 接下来

- [ruci-cmd程序](app/cmd.md)
- [订阅](app/subscrible.md)
- [lua配置](lua/lua.md)
- [路由配置](lua/route_config.md)
