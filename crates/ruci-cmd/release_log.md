
# v0.0.7

Dec 25, 2024

项目：
- 添加了 cc0 开源许可。
- 修复了 remote.lua 示例中一个致命错误。
- 修复了 用户手册的一个bug.
- 完善了用户手册。

ruci:
- 修复了 h1.1解析的一个bug。
- http 代理支持了客户端。
- Counter 支持了 udp。

rucimp:
- 添加了 spe1 （隐写示例协议1） 和 lua 用户自定义协议。
- 添加了一个用于记录流量的 Recorder Map.
- 添加了toml 格式的支持。
- 完全移除了旧verysimple 格式。

ruci-cmd:
- 支持了读取运行 tar 或 tar.zip 文件中的配置文件。
- utils 添加了 repl, pack, pack-z, serve-folder 子命令。
- 利用 pack 和 serve-folder 功能 提供了一种简易的订阅的方式。
- 修复了 Linux 发布版运行时提示缺少glibc 的问题。
- 修复了 windows gnu 发布版 运行闪退的问题。

# v0.0.6-beta.1
Aug 4, 2024

与 0.0.5 的不同

添加 ruci用户手册
https://e1732a364fed.github.io/ruci/index.html

ip流量转发功能 (类似vpn)，见 local.lua 中的 config_16_tun 配置. 其在 remote.lua 用到了 out_auto_route 配置，local.lua 中对应配置则改名为 in_auto_route了

添加一个有bug 的 tcp/ip stack

添加 dns 功能，Direct,BindDialer,OptDirect,OptDialer 都可以加

```lua
 Direct = {
        dns_client = {
            dns_server_list = { { "127.0.0.1:20800", "udp" } }, -- 8.8.8.8:53
            ip_strategy = "Ipv4Only",
            static_pairs = {
                ['www.baidu.com'] = "103.235.47.188"
            }
        }
    }
```

bind_to_device 支持 macOS

Stdio:+write_mode

# v0.0.5

Mar 27, 2024

与 0.0.4 的不同

+fixed_target_addr, quic, tproxy ( 包括自动路由)

与 beta.2的不同

ruci:
- 修复 Stdio 在 windows 上的报错问题
- 改进udp 的转发逻辑

rucimp:
- 修复 tproxy udp 卡顿问题。tproxy 功能已经通过了大量测试，已稳定。
- chain lua 配置格式发生变化： config -> Config
- dyn_selectors -> Dyn_Selectors
- infinite -> Infinite
- create_out_map -> Create_out_map
- create_in_map -> Create_in_map
- 优化代码

ruci-cmd: 
- 启用了 tun feature
- 日志打印代码行号



## v0.0.5-beta.2

Mar 25, 2024

与 v0.0.5-beta.1 的不同

- 配置支持 ipv6 字符串，如 tcp://[::1]:1080
- Dialer 更名为 BindDialer (配置文件中同样更名)
- Listener 支持 udp fixed_target_addr
- 修复 BindDialer 在使用 udp fixed_target_addr 时的问题
- 修复 一些情况下路由失败的问题

## v0.0.5-beta.1

Mar 24, 2024

ruci:
+fixed_target_addr for Listener, Dialer (same behavior as so called "Dokodemo door" in some other proxy apps)

rucimp:
- +quic, tproxy (See local.lua and remote.lua)
- +TcpOptListener, OptDialer, OptDirect, TproxyTcpResolver, TproxyUdpListener

ruci-cmd:
- feature: +tproxy, quic, quinn
- utils: +GenCer: generate self signed root key and certificate; + CalcuTrojanHash


# v0.0.4

## v0.0.4-beta.5
Mar 19, 2024

与 0.0.4-beta.4 的不同：

编译添加了更多目标支持

令 grpc client 默认行为 为 0rtt*, 使其与 现有其它代理程序的服务端兼容

修复了一个 危险的 trojan server 实现中的 无限循环的bug (在h2 连接发生错误的情况下会触发）

修复 http proxy 不可用的问题http 头配置中的

host项改名为 authority, 这样语义更清晰

注: 0rtt这里指建立客户端子连接无需等待 服务端回复即开始传输

## v0.0.4-beta.4
Mar 18, 2024

与 beta2 的不同
- 添加 H2Single 对 grpc 和 http_config 的支持，令其与 H2Mux 的配置格式相同
- 修复 native-tls 相关的 features 的条件编译问题
- 修复几个 grpc 的 bug
- 添加了 local_mux2_h2.lua 示例演示多连接的h2多路复用

与 beta3 的不同
- 添加 H2Single 对 grpc 和 http_config 的支持，令其与 H2Mux 的配置格式相同
- 修复1个 grpc重要 bug：大流量下如4k视频的情况下会 卡住


## v0.0.4-beta.2
Mar 17, 2024

修复一些小问题. 增加了对 aarch64 linux 和 aarch64 android 的支持

在启动时打印 启用的 feature

## v0.0.4-beta.1

Mar 16, 2024

0.0.4 版本是一个重要的版本，添加了若干项重要功能，令ruci变得比较实用:

ruci:

- fallback, 回落功能, 现支持 http proxy, socks5, trojan, http_filter. 回落配置可使用 fallback_route 项 或 rule_route 项
- cid 显示采用链式格式。如在 h2 mux 中，将 能见到如 1-22-33 形式的cid
- tls 的 apln 配置。
- more robust code for copying
- 更好的日志输出格式

rucimp:

- 完全动态链功能的升级，令完全动态链的速度大幅度提升
- websocket, 包括 early_data, 包括自定义 host, path 和 header
- http_filter, 用于对 websocket/grpc 等 进行回落，用法见 remote.lua
- native_tls,包括 alpn. 其中 native_tls 只支持客户端配置 alpn。native_tls的 配置内容和 普通tls 一样，只需要把 TLS = 更换成- NativeTLS = . 其目前只支持 PKCS8格式 的key
- h2, 包括 mux。单路outbound 配置为 "H2Single", 多路复用outbound 配置为 H2Mux
- grpc, 包括 mux。 通过设置 h2 配置的 is_grpc 来启用。用法见 local_mux_h2.lua 和 remote.lua （使用了 完全动态链功能 来进行多路复用）

ruci-cmd:

添加输出日志到文件功能，默认启用, 默认日滚动; 配置法见 --help, 可用 --log-dir 和 --log-file 配置。 输出的 日志文件的内容 同 verysimple 一样为 json 格式，便于进一步处理

bug fix for trojan early data reading

# v0.0.3-beta.1

Mar 10, 2024

- ruci-cmd 首版发布，不再发布 rucimp/examples
- ruci-cmd: api-server, api-client, utils, trace 四项可选feature, 发布版中 前三项是开启的

api内容请参阅 ruci-cmd

ruci-cmd的使用请用 --help 查阅，或参阅 notes 和 ruci-cmd

- +有限动态链、完全动态链配置，见 local.lua中相应示例
- +ruleset 路由配置支持
- +rucimp/examples 添加 chain_infinite, chain_trace


# v0.0.2
Feb 29, 2024

添加的功能均通过了测试并有配 resource/local.lua 和 remote.lua 中的相应配置示例。

+UDP, UDS, IP(tun)

    UDS流可由 Dialer和 Listener 发生; UDP和 IP 可由 Dialer 发生, 且此Dialer既可为 inbound 也可为 outbound。
    IP 要开启 tun feature

+UDP for trojan, socks5 .
+Blackhole, Echo, Fileio, Stdio Mapper
+rule_route, tag_route, 见 lua 示例

-移除了 suit 示例，并把 suit2 重命名为 suit
-移除了 static等不安全的用法, 使用 Arc

lua配置与0.0.1相比有一些变化, 比如 TcpListener 现在是 Listener, 因为一个 Listener 就可以兼顾 tcp 和 unix domain socket.
Dialer 同理，ip, tcp, udp, uds, 都用 Dialer 拨号。具体以示例为准

# 0.0.1
Feb 22, 2024

(No ruci-cmd yet, only rucimp/example)