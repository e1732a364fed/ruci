# dev note

链式模式的一个特点是，每一层都不知道上层和下层的确切信息，它只做自己层做的事

这会有一个现象：无法直接在 ws,tls,trojan,vless 等协议的outbound中直接传递early data , 因为
early data 必须由最末端的 outbound 传递

不过，我们可以在静态配置情况下做些操作，给末端代理一个标记，这样就能使用 earydata 功能了.

通过使用 CommonMapperExt 和 DefaultMapperExt 这两个derive 宏, 可以分别给 struct 实现 common行为
和默认行为.

CommonMapperExt 要 配合 common_mapper_field 宏一起使用
