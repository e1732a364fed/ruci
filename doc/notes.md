# user note 

链式配置中，每条链都必须标一个 tag

## tls

### 证书

tls 中， native_tls 只支持 pks8 和 pks12 两种格式, 而 ruci 中目前又只写了pks8 一种情况(即不支持 rsa 和 ecc key);

而默认的 rustls 则支持得更广泛一些,pem格式的 x509证书（后缀可能为 pem, cer 或 crt）, key(rsa, pks8, ecc) 都支持 ，但不支持 pks12 (pfx) 格式

### alpn 的行为


server:
Protocol names we support, most preferred first. If empty we don't do ALPN at all.

client:
Which ALPN protocols we include in our client hello. If empty, no ALPN extension is sent

如果任意一方的alpn 都没给出，则连接都通过；如果两方 alph 都给出，则只有匹配了才通过

native-tls 的 server 不支持手动设置 alpn



## 名词

在 suit 模式中, 使用 server, client 这样的形式, 而在 chain 模式中, 使用 inbound 和 
outbound 的形式. 这两者是一样的功能, 只是由于抽象的程度不同, 因此叫法不同

在 suit 模式中, server 的行为是 listen, client 的行为是 dial; 而在 chain 模式中, inbound
和 outbound 行为都叫做 map (映射) 

### 动态链

动态链分 有限动态链和 完全动态链

动态链的 iter 每次调用时, 会动态地返回一种Mapper
只有运行时才能知晓一条链是由哪些 Mapper 所组成, 所以无法用 Vec等类型表示,
只能用 Iterator 表示

不过, 有时会有这种情况: 动态链由几部分 静态链组成, 其中两个静态链之间的连接
是动态的

这里将这种链叫做 "Partial/Finite Dynamic Chain", 把完全动态的链叫做
"Complete/Infinite Dynamic Chain"

Partial 的状态是有限的 (即有限状态机 FSM),  Complete 的状态是无限的,
(即无限状态机)

比如，一个 tcp 到一个 tls 监听 ，这部分是静态的，之后根据 tls 的 alpn 结果
，进行分支，两个子分支后面也是静态的，但这个判断是动态的

而完全动态链有最大的灵活性，能实现所有一般情况下无法实现的效果。它是图灵完备的

比如 分支, 多路复用, 负载均衡, 都可以用 完全动态链实现


## 常见配置问题


### 报错示例: socks5 client only support tcplike stream, got NoStream


注意几乎所有的 outbound 都要先有一个 "流发生器", 如 Dialer, 如果直接是 socks5/trojan 的话，
没有流发生器, 是无法建立任何连接的

也就是说, 要有一个拨号环节


# lib note

ruci中有三种 route 实现 fixed, tag, info; 而 rucimp 有一种完整的 route 实现: RuleSet


# dev note

## feature  

rucimp 中有很多feature :

lua, lua54, route,geoip, tun, sockopt, use-native-tls, native-tls-vendored



## is_tail_of_chain

链式模式的一个特点是，每一层都不知道上层和下层的确切信息，它只做自己层做的事

这会有一个现象：无法直接在 ws,tls,trojan,vless 等协议的outbound中直接传递early data , 因为
early data 必须由最末端的 outbound 传递

不过，我们可以做些操作，给末端代理一个标记，这样就能使用 earydata 功能了.

通过使用 MapperExt 和 NoMapperExt 这两个derive 宏, 可以分别给 struct 实现 common行为
和默认行为.

MapperExt 要 配合 mapper_ext_fields 宏一起使用

用了 MapperExt 后，可以在方法内使用 self.is_tail_of_chain 判断是否在链尾，如果在，则可以发送ed, 
如果不在，不可以发送，只能传递到下一级

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

在项目初期，对其实现做了多种尝试, 一开始使用 enum AnyData, 后来将 enum 分成
单体AnyData 和 Vec<AnyData> 两部分, 再后来尝试使用smallvec<[AnyData;1]>

最终使用了 #[typetag::serde] 的 trait 方式

最初是将动态数据 Arc<AtomicU64>也放在 enum 中，后来移出, 单独做处理, 因为
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

在0.0.3，添加 "trace" feature, 对每条连接加以监视、记录, 其可能导处性能下降, 但
又在另一些用例中有用，所以要做

trace 会将chain 中经过的每一个 Mapper的 name 记录下来, 放到 `Vec<String> ` 中. 它只对于动态链有用
如果是静态链, 则记录一个 chain_tag 就能知道完整的 链信息

trace 还会将 【每条连接】的【实时】 ub, db 信息记录下来，这是最耗性能的


## HttpFilter

为了支持对 普通 http 请求的回落，加一个 叫 HttpFilter 的 Mapper

这样可以同时在 grpc 和 ws 中使用

因为 tungstenite (websocket包) 对错误请求是自行返回 http 响应的，而我们为了回落到其它 Mapper ，
就要 绕过 tungstenite 的处理


## 其它

使用 anyhow 的 context 会导致变慢，若有初始化开销 则要改用 with_context

再比如，要用 ok_or_else, 而不是 ok_or

使用 mlua 跨线程时要用 Mutex锁, 否则mac 上报错会类似 zsh: trace trap 

上传和下载的缩写代码中使用了 ub, db, 而不是 tx, rx, 是为了简单地与 channel 的Sender和 Receiver的缩写加以区分,
而且还能看出是以字节为单位

链的发展顺序：静态链，有限动态链，无限动态链
