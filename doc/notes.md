# user note 

链式配置中，每条链都必须标一个 tag

## 名词

在 suit 模式中, 使用 server, client 这样的形式, 而在 chain 模式中, 使用 inbound 和 
outbound 的形式. 这两者是一样的功能, 只是由于抽象的程度不同, 因此叫法不同

在 suit 模式中, server 的行为是 listen, client 的行为是 dial; 而在 chain 模式中, inbound
和 outbound 行为都叫做 map (映射) 

# lib note

ruci中有三种 route 实现 fixed, tag, info; 而 rucimp 有一种完整的 route 实现: RuleSet


# dev note

## feature  

rucimp 中有很多feature :

"lua", "route","geoip"



## is_tail_of_chain

链式模式的一个特点是，每一层都不知道上层和下层的确切信息，它只做自己层做的事

这会有一个现象：无法直接在 ws,tls,trojan,vless 等协议的outbound中直接传递early data , 因为
early data 必须由最末端的 outbound 传递

不过，我们可以在静态配置情况下做些操作，给末端代理一个标记，这样就能使用 earydata 功能了.

通过使用 CommonMapperExt 和 DefaultMapperExt 这两个derive 宏, 可以分别给 struct 实现 common行为
和默认行为.

CommonMapperExt 要 配合 common_mapper_field 宏一起使用

用了 CommonMapperExt 后，可以在方法内使用 self.is_tail_of_chain 判断是否在链尾，如果在，则可以发送ed, 
如果不在，不可以发送，只能传递到下一级

而作为最高级抽象的动态链则做不到. 静态链是动态链的一种具体的固定的形态

## async


目前用起来tokio 和 async_std 的最大的区别是, tokio 的TcpStream 不支持 clone;
async_std的 UdpSocket 少了 poll 方法 (until 24.2.18)

## Cargo.lock

本来作为类库是不应该有 Cargo.lock 的, 但我们同时也发布 examples, 为保证其能正常编译, 还是提
供了 lock 文件



## 移除 static 借用的办法

最初开发时, 采用了在 golang 上相同的思路, 但后来发现会越来越多地用到 static 和
Box::leak, 进而进行手动管理内存, 这一定是有问题的. 

从 commit  58cb71013036c2eab3e4a6898f6b43a5ac822fa4 一直到
ba02e41a4f81e3cea9626a93f8cefd16a539e341

都是在做重构代码的工作.

具体移除的思路是, 

1. 让 MIter trait 使用 Arc<Box dyn>, 而不是 &'static dyn
2. 让 Engine 保有数据的所有权, 而不是使用借用.
3. 不该由Engine 长期持有的数据就不持有, 而是通过init方法参数进行一次性使用
4. Engine run的时候, 使用 Engine 数据的拷贝 而不是直接使用 Engine 数据本身; 如果是拷贝比较重, 就使用 Arc

这样, 就保证了每一不同生命周期的部分都有自己数据的所有权, 就不再需要 static

