
现在我们学习 Config 中几种 route 块的写法:

```lua

routes = {
    tag_route = {},
    fallback_route = {},
    clash_rules = "the_clash_rules.yaml",
    geosite = "mygeosite_file_name.mmdb",
    geosite_gfw = {}
}
```

所有 route 都会用到 chain 的 `tag`
所以我们要求每一条inbound 都要有一个 tag, 每一个 inbound 中的 chain 都要有至少一个 map


# tag_route

```lua
    tag_route = { { "l1", "d1" }, { "l2", "d2" }, { "l3", "d2" } },
```

tag_route 是一个 字符串对 的列表。对中前者为 inbound 的 tag, 后者为 outbound 的 tag。

这是一种固定的 路由模式，只要来自 l1 的 都会被发到 d1.

# fallback_route

首先学一下什么是 fallback. 

在一个 inbound的 chain 中，有序地排列着多个 InMapConfig, 即它们代表着多个 Map,
分别记为 map1, map2. 假设 map1 通过了，但 map2 的协议 逻辑检查失败，即 map2
检查数据，发现和 map2 对应的 协议所定义的 特征不一致，那么此时 整个 chain 就此中断。

如果就这样，一般的情况就是 在 log 中记录一下此次 异常情况, 然后继续 监听 其它请求。

但是，有时，map2 错了，我们依然认为 它是一个有效的 map1, 想要 将它转发到一个新的
outbound 上，此时就用到了 fallback_route. 这整个行为就叫 fallback.

没错。这个就是 trojan 协议的 精髓。


```lua
    fallback_route = { { "listen1", "fallback_dial1" } }

```

fallback_route 是一个 字符串对 的列表。上面示例就是表示 inbound chain "listen1" 里失败的地方将被转发到 
outbound chain "fallback_dial1" 中。listen1 和 fallback_dial1 是它们的 tag.

# clash_rules

0.0.8开始，ruci 支持了clash 的分流规则的使用。clash 是一个知名的app, 它的规则被用得很多


一般直接写为 clash_rules = "myfile.yaml"

然后在 myfile.yaml 中，按 clash 配置文件中的 rules 项进行书写，如写成：

```yaml
rules:
  - DOMAIN-SUFFIX,ip6-localhost,Direct
  - DOMAIN-SUFFIX,ip6-loopback,Direct
  - DOMAIN-SUFFIX,lan,Direct
  - DOMAIN-SUFFIX,local,Direct
  - DOMAIN-SUFFIX,localhost,Direct
  - DOMAIN-KEYWORD,baidu,Reject
```
就行了

ruci 支持所有 clash 中定义的 规则，而且有算法加速支持，匹配得很快！

您还可以直接把 yaml 的内容传入 clash_rules 项，但是不太建议这么做：

```lua
clash_rules = "rules:\n  - DOMAIN-SUFFIX,ip6-localhost,Direct"
```

好处就是可以一个配置文件全搞定

# geosite

配置 geosite 的 mmdb 是用于 clash_rules的。也就是说，配置了 geosite后，clash_rules中的 GEOSITE 规则就生效了

可以运行 ruci-cmd utils 中的 对应命令来下载 geosite 文件

# geosite_gfw

 https://github.com/e1732a364fed/geosite-gfw

```
geosite_gfw = {
    api_url = "http://127.0.0.1:5134/check",
    proxy = "127.0.0.1:10800",
    only_proxy = false,
    ok_ban_out_tag = { "Direct", "Reject"}
}
```

geosite_gfw 是一个 人工智能 gfw项目，它用过机器学习训练出的模型来判断一个 域名 倒底是会被墙还是 可以直连

主要用于 local 本地端进行分流。

proxy 选项若给出，则 geosite_gfw 会在访问不到目标地址时，使用 proxy 再访问一次。
而 若 only_proxy = true, 则 geosite_gfw 第一次方问目标地址就会使用 该 proxy.

注意，如果您配置 geosite_gfw 的 proxy 又指向回 我们的 ruci 的监听端口的话，要确保按上文的 
tag_route 把该端口的监听强制导向某 outbound, 避免再次进竹 geosite_gfw 环节 造成 回环。


目前的geosite_gfw 的运行方式:

```sh
git clone https://github.com/e1732a364fed/geosite-gfw/
cd geosite-gfw
pip3 install transformers numpy scikit-learn flask requests
pip3 install torch

curl -LO "https://huggingface.co/e1732a364fed/geosite-gfw/resolve/main/bert_geosite_by_body.zip?download=true"
curl -LO "https://huggingface.co/e1732a364fed/geosite-gfw/resolve/main/bert_geosite_by_head.zip?download=true"

tar -xf bert_geosite_by_body.zip
tar -xf bert_geosite_by_head.zip

python3 classify.py --mode serve_api --port 5134
```

之后在 ruci 的 routes 中使用 geosite_gfw 就能生效啦。



# 接下来

学点难的？
[Infinite](infinite.md)
