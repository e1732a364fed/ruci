# user note 

链式配置中，每条链都必须标一个 tag


# dev note

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
