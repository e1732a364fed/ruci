# user note 

链式配置中, 每条链都必须标一个 tag

## tls

### 证书

tls 中,  native_tls 只支持 pks8 和 pks12 两种格式, 而 ruci 中目前又只写了pks8 一种情况(即不支持 rsa 和 ecc key);

而默认的 rustls 则支持得更广泛一些,pem格式的 x509证书（后缀可能为 pem, cer 或 crt）, key(rsa, pks8, ecc) 都支持 , 但不支持 pks12 (pfx) 格式

除了用 ruci-cmd utils gen-cer 命令生成自签名根证书, 还可以试图自行用 openssl 命令生成:

生成 key 和 证书:

```sh
# ec key
openssl ecparam -genkey -name prime256v1 -out cert.key
openssl req -new -x509 -days 7305 -key cert.key -out cert.pem

# rsa key
openssl req -x509 -sha256 -newkey rsa:4096 -keyout test2.key -out test2.crt -days 7305
openssl rsa -in test2.key -out test2.key


# 生成自签 根证书 (root-req.csr 为中间产物)

openssl genrsa -out root.key 2048
openssl req -new -out root-req.csr -key root.key -keyform PEM
openssl x509 -req -in root-req.csr -out root-cert.cer -signkey root.key -CAcreateserial -days 3650  -extfile ext.ini
```

ext.ini:

```ini
basicConstraints = CA:FALSE
keyUsage = nonRepudiation, digitalSignature, keyEncipherment
subjectAltName = @alt_names
 
[alt_names]
DNS.1 = www.mytest.com
DNS.2 = localhost
```



### alpn 的行为


server:
Protocol names we support, most preferred first. If empty we don't do ALPN at all.

client:
Which ALPN protocols we include in our client hello. If empty, no ALPN extension is sent

如果任意一方的alpn 没给出, 则连接都通过；如果两方 alph 都给出, 则只有匹配了才通过

native-tls 的 server 不支持手动设置 alpn

## tproxy

使用 tproxy 时, 确保是 linux 系统, 并 安装了 iptables (`apt install iptables`)

若要 代理 局域网内其它设备, root 权限运行

```sh
echo net.ipv4.ip_forward=1 >> /etc/sysctl.conf && sysctl -p
```

该命令确保 /etc/sysctl.conf 文件中 包含 `net.ipv4.ip_forward=1` 且生效



## 名词

在 suit 模式中, 使用 server, client 这样的形式, 而在 chain 模式中, 使用 inbound 和 
outbound 的形式. 这两者是一样的功能, 只是由于抽象的程度不同, 因此叫法不同

在 suit 模式中, server 的行为是 listen, client 的行为是 dial; 而在 chain 模式中, inbound
和 outbound 行为都叫做 map (映射) 

### 动态链

动态链分 有限动态链和 完全动态链

动态链的 iter 每次调用时, 会动态地返回一种Map
只有运行时才能知晓一条链是由哪些 Map 所组成, 所以无法用 Vec等类型表示,
只能用 Iterator 表示

不过, 有时会有这种情况: 动态链由几部分 静态链组成, 其中两个静态链之间的连接
是动态的

这里将这种链叫做 "Partial/Finite Dynamic Chain", 把完全动态的链叫做
"Complete/Infinite Dynamic Chain"

Partial 的状态是有限的 (即有限状态机 FSM),  Complete 的状态是无限的,
(即无限状态机)

比如, 一个 tcp 到一个 tls 监听 , 这部分是静态的, 之后根据 tls 的 alpn 结果
, 进行分支, 两个子分支后面也是静态的, 但这个判断是动态的

而完全动态链有最大的灵活性, 能实现所有一般情况下无法实现的效果. 它是图灵完备的

比如 分支, 多路复用, 负载均衡, 都可以用 完全动态链实现


## 常见配置问题

### udp 的监听

ruci 新手常见的错误, 用 Listener 监听 udp

Listener的原理是 listen  然后 accept  出 子流，而udp 是不会分出子流的

所以 udp 监听要用的是 BindDialer

监听 udp 本地 20800 端口:

```lua

BindDialer = {
    bind_addr = "udp://127.0.0.1:20800"
}
```

### BindDialer 的 udp fixed_target_addr 转发 问题

fixed_target_addr (dokodemo) 对于 udp BindDialer 有一个问题

比如试图监听一个 20800 端口， 作为一个 dns 服务转发

只有一个 client 询问 dns 时， 如 client1 问 www.1.com 的 ip 是什么，

然后我们 20800 收到后，问 实际 dns, 得到答后，回复给 client1

这没问题。

但

假设有 client1, client2 同时问问题， client1 问 www.1.com, client2 问 www.2.com

然后我们都得到了实际dns 的答，但回复时，如何知道

www.1.com 的答 是回复给 client1  还是 client2 呢？

注意 实际dns 的答 是不一定按原问的顺序的

注意 我们是不会探查 udp 的内容并记录的


举个形象的例子

一个课代表收作业，收了很多作业，作业有记名，老师批完了，发回课代表，但发回课代表的不是批过的作业，而是一个个 成绩，请问课代表如何 发回 作业作者？

不完美方案：

1. 记录连接的顺序，然后按回答顺序回复给源。（如，记录交作业的顺序，按出成绩的顺序 回复）。问题：出成绩的顺序与交作业的顺序不一定一致
2. 探查 udp 内容，按答案中的内容匹配后回复给源。问题：这样属于侵犯源的隐私, 且若不为已知协议则做不到
3. 将回复 广播给所有 源。问题：这样属于侵犯源的隐私
4. 独占性: 让用户自行确保同一段时间内只有唯一的客户端连接 ruci的 BindDialer


都不完美. 绕过的方案是: 不用 BindDialer 做 udp 转发, 而是 使用 Listener

Listener 在 监听 udp, 且 有 udp 的 fixed_target_addr 时, 会对每一个 inbound
连接新建一个 udp 连接 , 建立了一对一的转发, 而不是 一对多的转发, 就没问题了

有用户报告 用 BindDialer 的 fixed_target_addr 作 udp 转发会导致宕机, 所以一定要用 Listener


### 报错示例: socks5 client only support tcplike stream, got NoStream


注意几乎所有的 outbound 都要先有一个 "流发生器", 如 BindDialer, 如果直接是 socks5/trojan 的话, 
没有流发生器, 是无法建立任何连接的

也就是说, 要有一个拨号环节

## 与 verysimple 配置 (即 ruci中的 suit 模式, toml 格式配置文件) 的对比

verysimple 有几个不清不楚的地方: 
1. trojan 的 password 写在了 uuid 里
2. grpc 的 service name 填在了 path 中, 然后没有 / 前缀; 但 ws写的path却要有 /, 
3. vs 的 host 既用于 tls 的 sni, 又用于 websocket/grpc 的 http 请求中的 host (其实是 uri 中的authority, 包含端口号), 但实际上二者可以不同


这些方面 ruci 分得更清楚, 因为用了链式架构

ruci chain 模式中,  

1. trojan 的 password 写在自己配置中的 password 项里
2. grpc和 h2一样的, 没有 service name 一说, path直接写为 /service1/Tun 即可
3. tls 的 sni 写的 tls 的配置中, ws/grpc 的 authority 写在 它们自己的配置中
4. vs 中的 ws server 要加 early = true 才能使持 earlydata, 而 ruci 中的 ws server 是默认支持的, 只需要在 ws client 端打开use_early_data

在ruci 中, 你可以:  dial 一个由 host1 解析得的ip, 然后 tls 里的 sni 写 host2, 然后 ws/grpc 的请求 url 中 写 host3


# lib note

ruci中有三种 route 实现 fixed, tag, info; 而 rucimp 有一种完整的 route 实现: RuleSet


# dev note

## feature  

rucimp 中有很多feature :

lua, lua54, route,geoip, tun, sockopt, use-native-tls, native-tls-vendored, quinn, quic



## is_tail_of_chain

链式模式的一个特点是, 每一层都不知道上层和下层的确切信息, 它只做自己层做的事

这会有一个现象: 无法直接在 ws,tls,trojan,vless 等协议的outbound中直接传递early data , 因为
early data 必须由最末端的 outbound 传递

不过, 我们可以做些操作, 给末端代理一个标记, 这样就能使用 earydata 功能了.

通过使用 MapExt 和 NoMapExt 这两个derive 宏, 可以分别给 struct 实现 common行为
和默认行为.

MapExt 要 配合 map_ext_fields 宏一起使用

用了 MapExt 后, 可以在方法内使用 self.is_tail_of_chain 判断是否在链尾, 如果在, 则可以发送ed, 
如果不在, 不可以发送, 只能传递到下一级

## async


目前用起来tokio 和 async_std 的最大的区别是, tokio 的TcpStream 不支持 clone;
async_std的 UdpSocket 少了 poll 方法 (until 24.2.18)

## Cargo.lock

本来作为类库是不应该有 Cargo.lock 的, 但我们同时也发布 examples, 
为保证其能正常编译, 还是提供了 lock 文件


## 移除 static 借用的办法

最初开发时, 采用了在 golang 上相同的思路, 但后来发现会越来越多地用到 static 和
Box::leak, 进而进行手动管理内存, 这一定是有问题的. 

从 commit  58cb71013036c2eab3e4a6898f6b43a5ac822fa4 一直到
ba02e41a4f81e3cea9626a93f8cefd16a539e341

都是在做重构代码的工作.

具体移除的思路是, 

1. 让 MIter trait 使用 `Arc<Box dyn>`, 而不是 `&'static dyn`
2. 让 Engine 保有数据的所有权, 而不是使用借用.
3. 不该由Engine 长期持有的数据就不持有, 而是通过init方法参数进行一次性使用
4. Engine run的时候, 使用 Engine 数据的拷贝 而不是直接使用 Engine 数据本身; 如果是拷贝比较重, 就使用 Arc

这样, 就保证了每一不同生命周期的部分都有自己数据的所有权, 就不再需要 static

## MapResult 中的 "任意数据类型"

在项目初期, 对其实现做了多种尝试, 一开始使用 enum AnyData, 后来将 enum 分成
单体AnyData 和 Vec<AnyData> 两部分, 再后来尝试使用smallvec<[AnyData;1]>

最终使用了 #[typetag::serde] 的 trait 方式

最初是将动态数据 Arc<AtomicU64>也放在 enum 中, 后来移出, 单独做处理, 因为
动态数据不能也不应该做序列化

## maxmind db 的 geoip

在(24.2.28)测试中发现, 最新的 mmdb ,
从 https://github.com/Loyalsoldier/geoip/releases 下载的,

如 202402220055, 202310260055 , 202301050111 中, 它对一些知名互联网公司的 ip 的 iso 的返回值是 特殊的值, 如 GOOGLE, TWITTER

重新从其下载旧的 mmdb, 发现旧的  202203250801,  202209150159
 版是正常的 ( 返回值为 US)

这说明, mmdb 的的文件内容在2022年9月以后, 23年1月 以前 的某个时间上 发生了变化. (没全测, 时间有限)

不过这些公司应该都是美国的

我想这应该就是 Loyalsoldier/geoip 的readme 中说明了 添加了 "geoip:cloudflare" 等类别的原因

### test 相关

为了在github action 通过 测试, 将 需要 Country.mmdb 的几个 test 注释掉了

## "trace" feature

在0.0.3, 添加 "trace" feature, 对每条连接加以监视、记录, 其可能导处性能下降, 但
又在另一些用例中有用, 所以要做

trace 会将chain 中经过的每一个 Map的 name 记录下来, 放到 `Vec<String> ` 中. 它只对于动态链有用
如果是静态链, 则记录一个 chain_tag 就能知道完整的 链信息

trace 还会将 【每条连接】的【实时】 ub, db 信息记录下来, 这是最耗性能的


## HttpFilter

为了支持对 普通 http 请求的回落, 加一个 叫 HttpFilter 的 Map

这样可以同时在 grpc 和 ws 中使用

因为 tungstenite (websocket包) 对错误请求是自行返回 http 响应的, 而我们为了回落到其它 Map , 
就要 绕过 tungstenite 的处理

## quic 

使用了 s2n-quic 包 或 quinn 包

使用 quic 会给 ruci-cmd release 加 3-4MB 大小左右


### quic 的两种不同实现的 多代理程序  互通性：
 
截至 23.3.21

s2n-quic:

ruci 自己连自己，没问题
ruci 作 客户端， verysimple 作 服务端，能通，能过trojan 得 target_addr, 但之后 relay 阶段卡住
ruci 作 服务端， vs 作客户端，连不上，就像没运行ruci 一样.


这个行为 在 s2n-quic 中使用 rustls 与 使用 s2n-tls 的效果是一样的

quinn: 全没问题

故 ruci 默认使用 quinn 作为 quic实现。

（两个依赖包的接口代码几乎是相同的，只能说明s2n-quic 互通性还不完善)

而且 s2n-quic 在 windows 无法编译通过

## 编译运行问题

tproxy,tun 要使用sudo 运行

### 1
`panic = "abort"` 不能在 windows release 版中正常运行

### 2
windows上运行 gnu 版会报 应用程序无法正常启动, 0xc00007b

这是因为它依赖 libstdc++-6.dll

英文:

`The application was unable to start correctly (0xc000007b).`

### 3
linux release 使用gnu 版可能会报 glibc 问题, 解决方法是

1. 更新系统的glibc或 
2. 使用 musl 版
3. 自己编译

更新系统的 glibc 是比较危险的做法, 推荐使用 musl



## 其它

使用 anyhow 的 context 会导致变慢, 若有初始化开销 则要改用 with_context

再比如, 要用 ok_or_else, 而不是 ok_or

使用 mlua 跨线程时要用 Mutex锁, 否则mac 上报错会类似 zsh: trace trap 

上传和下载的缩写代码中使用了 ub, db, 而不是 tx, rx, 是为了简单地与 channel 的Sender和 Receiver的缩写加以区分,
而且还能看出是以字节为单位

链的灵活顺序: 静态链, 有限动态链, 无限动态链
