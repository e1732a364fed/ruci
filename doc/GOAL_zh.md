# 终极目标 - AI协议

ruci项目的终极目标是利用人工神经网络针对用户自己的流量特点创建自定义的“AI协议”, 以保证安全性. 

在项目初期, 工作主要围绕下面任务展开: 基本架构的建设, 基本协议的实现 和用户流量数据的收集和标注(用户自己的上网行为就会产生数据, 数据标注后就可被机器学习, 数据完全放在用户本地）. 

原理是, 为了穿过【可能的】墙, 我们首先要造一个墙出来, 我们的第一步是, 用机器学习来判断用户的流量属于什么流量. 第二步是, 用机器学习做一个“AI协议”, 让自己的墙去看它是什么流量. 如果自己的墙分不出AI协议与普通上网流量的区别, 我们就赢了. 

用 pytorch 设计神经网络, 分为 ruciwall 和 ruciprotocol 两部分. python 项目叫 ruci-py.

尽管AI协议还只是概念阶段, 但第一步 自造一个墙 已经被实现. 
而AI协议已经被证明可以实现。

## 为什么？

笔者不擅长制作新协议, 也不擅长去人工分析某协议是否安全, 因此打算诉诸人工智能. 

总有一些PoC出现, 证明这样或那样的协议不安全, 那么为什么我们不能做一个通用的呢？
而且更进一步地逆向思考，做出能通过wall的AI协议

## 优点

你实现的协议越多, ruciwall就越强大, AI协议就越强大. 而且数据完全来自你的行为本身, ruciwall和ruciprotocol都是为你的上网行为量身定制的. 

## 缺点

你需要自行采集数据, 自行训练网络, 项目本身暂不会为你提供任何生成好的东西. 好在算不上深度学习，很快就能训练好ruciwall
不过, 不用AI协议的话, 完全可以拿本项目退化作为一个日常代理使用

## 较准确的描述

我们要解决的问题是, 

一个数据流(任意字节长度的数据), 流内含任意信息（称为D）, 流可以有不同的表达方式, 无论表达方式如何, 它都能原样转换为原流

以一部分数据确定表达方式, 并以另一部分数据表达D 的规则 被称为 ”协议“(P)

对于任意一个 P1 + D1, 是否有 另一个协议 P2, 传输数据 D2, 无法区分出 P1+D1 和 P2+D2 的区别, 
即 从流上来看, 实际使用的P是P1的概率 与 P是P2的概率相等
(P1!=P2, D1!=D2)

该问题有很多”弱化版本“, 比如, 不要求对任意P1+D1, 只要求对 TLS+D1
再弱化, 如, 是否有一个协议 P2, 传输数据 D2=TLS+D1,  无法区分出 P2+D2 和 TLS+D3 的区别, 

已经可以证明，存在一个完美安全的代理协议。(There exists a Provably Secure Steganography Proxy Protocol.)

# 相关领域

Deep Packet Inspection,数据分析,加密流量识别, 流量分类, 流量分析, 异常流量检测

Steganography, cryptography, Provable Perfect Security ， Covert Communication, network traffic generation, UGC( user generated content)

信息隐藏，隐写，隐写分析，隐敝通信

有很多大量的论文和相关的开源项目存在. 很多的流量识别网络有国家（中国）专利
总之识别异常流量已经是数年前的老内容了，在今天属于入门级的内容（在学习神经网络1天后即应具备识别异常流量的能力）。
我们主要更关心的是AI协议（生成式隐写）的问题

https://patents.google.com/patent/CN101741744B/zh
(接着看 Cited By 即可找到更多)

更多讨论可移步电报

## 可能有用的论文

https://www.usenix.org/conference/usenixsecurity24/presentation/xue-fingerprinting

Perfectly Secure Steganography Using Minimum Entropy Coupling
https://arxiv.org/abs/2210.14889

Deep Packet: A Novel Approach For Encrypted Traffic Classification Using Deep Learning
https://arxiv.org/abs/1709.02656


可证安全隐写：理论、应用与展望
https://www.journalofcybersec.com/CN/Y2023/V1/I1/38

生成式隐写研究
http://cjc.ict.ac.cn/online/bfpub/zlz-202322140707.pdf


Infranet: Circumventing Web Censorship and Surveillance
https://www.usenix.org/legacy/events/sec02/feamster/feamster.pdf


StegoTorus: A Camouflage Proxy for the Tor Anonymity
System
https://raw.githubusercontent.com/SRI-CSL/Stegotorus/refs/heads/master/doc/stegotorus.pdf

SkypeMorph: Protocol Obfuscation for Tor Bridges
https://cacr.uwaterloo.ca/techreports/2012/cacr2012-08.pdf

SWEET: Serving the Web by
Exploiting Email Tunnels
http://caesar.web.engr.illinois.edu/papers/sweet-ton17.pdf

NetDiffus: Network Traffic Generation by Diffusion
Models through Time-Series Imaging
https://arxiv.org/abs/2310.04429

TrafficGPT: Breaking the Token Barrier for
Efficient Long Traffic Analysis and Generation
https://arxiv.org/pdf/2403.05822

NetGPT: Generative Pretrained Transformer for
Network Traffi
https://arxiv.org/pdf/2304.09513

Lens: A FOUNDATION MODEL FOR NETWORK TRAFFIC
https://arxiv.org/pdf/2402.03646

NetBench: A Large-Scale and Comprehensive Network Traffic Benchmark Dataset for Foundation Models
https://arxiv.org/html/2403.10319v1

CloudTransport: Using Cloud Storage for Censorship-Resistant Networking
https://link.springer.com/chapter/10.1007/978-3-319-08506-7_1



## 一些项目与资源


https://github.com/munhouiani/Deep-Packet
https://blog.munhou.com/2020/04/05/Pytorch-Implementation-of-Deep-Packet-A-Novel-Approach-For-Encrypted-Tra%EF%AC%83c-Classi%EF%AC%81cation-Using-Deep-Learning/

https://github.com/mrazimi99/deep-packet

https://github.com/Srinivas11789/PcapXray

https://wiki.wireshark.org/SampleCaptures

Dust: A Polymorphic Engine for Filtering-Resistant Transport Protocols 
https://github.com/blanu/Dust

marionette
https://github.com/marionette-tg/marionette



## 学术刊物

中国计算机学会推荐国际学术刊物
https://www.ccf.org.cn/Academic_Evaluation/NIS/

IEEE Transactions on Dependable and Secure Computing(TDSC)
https://dblp.uni-trier.de/db/journals/tdsc/index.html

IEEE Transactions on Information Forensics and Security(TIFS)
https://dblp.uni-trier.de/db/journals/tifs/index.html

ACM Workshop on Information Hiding and Multimedia Security (IH&MMSec)
https://dblp.org/db/conf/ih/index.html


电子与信息学报
https://jeit.ac.cn/

应用科学学报
https://www.jas.shu.edu.cn/

信息安全学报
http://jcs.iie.ac.cn/xxaqxb/ch/index.aspx

网络空间安全科学学报
https://www.journalofcybersec.com/CN/home

计算机学报
http://cjc.ict.ac.cn/

等，以及各大学的学报
